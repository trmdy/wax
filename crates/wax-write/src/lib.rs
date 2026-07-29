//! Model → file writers (W4).
//!
//! The writer consumes a [`WorkbookStore`] — the same long-lived
//! representation `wax serve` holds — so exports work identically over the
//! protocol and from the CLI (which builds a store from a normalized
//! `Document` first). Export-a-copy semantics: the output is a new file
//! derived from the model; nothing is ever edited in place, and any model
//! feature the writer cannot express is reported loudly in
//! [`ExportOutcome::dropped`], never silently discarded.
//!
//! # The frozen W4 seam (`docs/w4-contracts.md` §2)
//!
//! [`write_xlsx`] and [`write_csv`] are the coordinator-frozen API that
//! `wax-cli` (serve `export` op + `wax export` subcommand) and the harness
//! round-trip validation build against. Changing these signatures requires a
//! coordinator amendment; additive helpers are welcome.

use std::collections::{HashMap, HashSet};
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader as XmlReader, Writer as XmlWriter};
use rust_xlsxwriter::{
    Color, ExcelDateTime, Format, FormatUnderline, Formula, Workbook, Worksheet, XlsxError,
};
use tempfile::{Builder as TempFileBuilder, NamedTempFile};
use wax_core::{CellStyle, CellType, CellValue};
use wax_store::{WindowCell, WorkbookStore};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

const CSV_DROPPED: [&str; 5] = [
    "formulas (cached values only)",
    "number formatting beyond display strings",
    "merges",
    "styles",
    "column widths",
];
const DEFAULT_DATE_FORMAT: &str = "yyyy-mm-dd";
const DEFAULT_DATETIME_FORMAT: &str = r#"yyyy-mm-dd"T"hh:mm:ss"#;
const DEFAULT_DATETIME_MILLIS_FORMAT: &str = r#"yyyy-mm-dd"T"hh:mm:ss.000"#;
const DEFAULT_TIME_FORMAT: &str = "hh:mm:ss";
const DEFAULT_TIME_MILLIS_FORMAT: &str = "hh:mm:ss.000";
const UNREPRESENTABLE_MERGES_DROPPED: &str = "unrepresentable merge ranges";
const CLAMPED_COLUMN_WIDTHS_DROPPED: &str = "column widths clamped to 0..=255";
const XLSX_MAX_STRING_CHARS: usize = 32_767;
const MAX_DROPPED_DETAILS: usize = 100;

/// A successful export: bytes written to the output file plus every feature
/// of the model (or of the source, when the caller merges open-time
/// warnings) that the export does not preserve. `dropped` entries are short
/// human-readable phrases, e.g. `"pivot caches"`, `"cell borders"`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportOutcome {
    pub bytes: u64,
    pub dropped: Vec<String>,
}

/// A structured writer failure. `code` uses the proto v0 `ErrorCode`
/// spellings (`"internal"`, `"bad_request"`, `"cancelled"`, ...) so callers
/// can pass it straight onto the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteError {
    pub code: String,
    pub msg: String,
}

impl WriteError {
    pub fn new(code: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            msg: msg.into(),
        }
    }
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.msg)
    }
}

impl std::error::Error for WriteError {}

/// Write the whole workbook as a styled xlsx copy: values, types, formula
/// text with cached results, number formats, merges, explicit column widths,
/// and basic cell styles. `cancel` is checked at row-granularity
/// checkpoints; a cancelled export returns `code: "cancelled"` and leaves no
/// partial output file behind.
///
pub fn write_xlsx(
    store: &WorkbookStore,
    out: &Path,
    cancel: &AtomicBool,
) -> Result<ExportOutcome, WriteError> {
    checkpoint(cancel)?;
    if store.sheet_count() == 0 {
        return Err(WriteError::new("bad_request", "empty workbook"));
    }

    let mut workbook = Workbook::new();
    let mut formats = HashMap::new();
    let mut dropped = Dropped::default();
    let merge_format = Format::new();
    let mut formula_patches = Vec::new();
    let mut used_sheet_names = HashSet::new();

    for sheet_index in 0..store.sheet_count() {
        checkpoint(cancel)?;
        let meta = store.sheet_meta(sheet_index).ok_or_else(|| {
            WriteError::new(
                "internal",
                format!("sheet index {sheet_index} disappeared during xlsx export"),
            )
        })?;
        if meta.truncated {
            dropped.add("source truncated at read time; export is the truncated model");
        }

        let sheet_name = unique_xlsx_sheet_name(&meta.name, &mut used_sheet_names);
        if sheet_name != meta.name {
            dropped.add(format!(
                "sheet name '{original}' sanitized to '{sanitized}'",
                original = meta.name,
                sanitized = sheet_name,
            ));
        }

        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name(&sheet_name)
            .map_err(|error| xlsx_error("set worksheet name", out, error))?;
        // An empty default cached result preserves formula cells whose source
        // file did not carry a cached value. Formula instances with a cached
        // value override this default below.
        worksheet.set_formula_result_default("");

        for col_info in store
            .sheet_col_infos(sheet_index)
            .expect("sheet metadata and column metadata must agree")
        {
            checkpoint(cancel)?;
            let width = clamp_column_width(col_info.width);
            if !col_info.width.is_finite() || width != col_info.width {
                dropped.add(CLAMPED_COLUMN_WIDTHS_DROPPED);
            }
            let column = xlsx_column(col_info.c)?;
            worksheet
                .set_column_width(column, width)
                .map_err(|error| xlsx_error("set column width", out, error))?;
        }

        // rust_xlsxwriter's merge_range() writes a string into the anchor.
        // Create the blank merges first, then let scan_sheet() overwrite each
        // populated anchor with its original typed value.
        for range in store
            .sheet_merges(sheet_index)
            .expect("sheet metadata and merge metadata must agree")
        {
            checkpoint(cancel)?;
            let Some((first_row, first_col, last_row, last_col)) = parse_a1_range(&range) else {
                dropped.add(UNREPRESENTABLE_MERGES_DROPPED);
                continue;
            };
            if (first_row, first_col) == (last_row, last_col)
                || worksheet
                    .merge_range(first_row, first_col, last_row, last_col, "", &merge_format)
                    .is_err()
            {
                dropped.add(UNREPRESENTABLE_MERGES_DROPPED);
            }
        }

        let mut last_row = None;
        let mut scan_error = None;
        let mut sheet_formula_patches = Vec::new();
        let mut previous_position = None;
        let mut previous_had_formula = false;
        let scanned = store.scan_sheet(sheet_index, |row, column, cell| {
            if scan_error.is_some() {
                return;
            }
            if last_row != Some(row) {
                last_row = Some(row);
                if let Err(error) = checkpoint(cancel) {
                    scan_error = Some(error);
                    return;
                }
            }
            if let Err(error) = xlsx_row(row) {
                scan_error = Some(error);
                return;
            }
            let position = (row, column);
            if previous_position == Some(position) && previous_had_formula {
                sheet_formula_patches.pop();
            }
            previous_position = Some(position);
            previous_had_formula = cell.f.is_some();
            let cached_formula = if let Some(formula) = &cell.f {
                let Ok(column) = xlsx_column(column) else {
                    scan_error = Some(WriteError::new(
                        "internal",
                        format!("column index {column} exceeds the xlsx limit"),
                    ));
                    return;
                };
                let cached_result =
                    match formula_result(&cell, &meta.name, row, column, &mut dropped) {
                        Ok(result) => result,
                        Err(error) => {
                            scan_error = Some(error);
                            return;
                        }
                    };
                Some((formula.clone(), column, cached_result))
            } else {
                None
            };
            if let Err(error) = write_xlsx_cell(
                worksheet,
                &mut formats,
                store.styles(),
                &meta.name,
                row,
                column,
                &cell,
                cached_formula
                    .as_ref()
                    .and_then(|(_, _, result)| result.as_deref()),
                &mut dropped,
                out,
            ) {
                scan_error = Some(error);
                return;
            }
            if let Some((formula, column, cached_result)) = cached_formula {
                sheet_formula_patches.push(FormulaPatch {
                    cell: cell_reference(row, column),
                    formula,
                    cached_result,
                    cell_type: cell.t,
                });
            }
        });
        if !scanned {
            return Err(WriteError::new(
                "internal",
                format!("sheet index {sheet_index} disappeared during xlsx export"),
            ));
        }
        if let Some(error) = scan_error {
            return Err(error);
        }
        formula_patches.push(sheet_formula_patches);
    }

    checkpoint(cancel)?;
    let mut temp = temporary_output(out, ".xlsx")?;
    workbook
        .save(temp.path())
        .map_err(|error| xlsx_error("save workbook", out, error))?;
    checkpoint(cancel)?;
    temp = normalize_formula_xml(temp, out, &formula_patches, cancel)?;
    temp.as_file()
        .sync_all()
        .map_err(|error| io_error("sync temporary xlsx output", out, error))?;
    checkpoint(cancel)?;
    let bytes = temp
        .as_file()
        .metadata()
        .map_err(|error| io_error("inspect temporary xlsx output", out, error))?
        .len();
    persist_output(temp, out)?;

    Ok(ExportOutcome {
        bytes,
        dropped: dropped.finish(),
    })
}

/// Write one sheet as CSV: RFC 4180 quoting, UTF-8, CRLF line endings. Each
/// cell emits its display string when present, else the raw value (numbers
/// via shortest round-trip, booleans as TRUE/FALSE, error text as-is).
/// Semantics must stay identical to the serve `export` CSV path shipped in
/// W3, which W4 moves here. A bad `sheet` index returns `bad_request`.
pub fn write_csv(
    store: &WorkbookStore,
    sheet: u32,
    out: &Path,
    cancel: &AtomicBool,
) -> Result<ExportOutcome, WriteError> {
    let meta = store.sheet_meta(sheet).ok_or_else(|| {
        WriteError::new(
            "bad_request",
            format!("sheet index {sheet} is out of range"),
        )
    })?;
    checkpoint(cancel)?;

    let mut temp = temporary_output(out, ".csv")?;
    {
        let mut output = BufWriter::new(temp.as_file_mut());
        let mut next_output_row = 0_u32;
        let mut pending_row = None;
        let mut pending_cells = Vec::new();
        let mut scan_error = None;

        let scanned = store.scan_sheet(sheet, |row, column, cell| {
            if scan_error.is_some() {
                return;
            }

            if pending_row != Some(row) {
                if let Some(previous_row) = pending_row.take() {
                    if let Err(error) =
                        write_csv_row(&mut output, meta.cols, &pending_cells, cancel, out)
                    {
                        scan_error = Some(error);
                        return;
                    }
                    pending_cells.clear();
                    next_output_row = previous_row.saturating_add(1);
                }

                while next_output_row < row {
                    if let Err(error) = write_csv_row(&mut output, meta.cols, &[], cancel, out) {
                        scan_error = Some(error);
                        return;
                    }
                    next_output_row = next_output_row.saturating_add(1);
                }
                pending_row = Some(row);
            }

            if let Some((last_column, last_cell)) = pending_cells.last_mut() {
                if *last_column == column {
                    *last_cell = cell;
                    return;
                }
            }
            pending_cells.push((column, cell));
        });
        if !scanned {
            return Err(WriteError::new(
                "bad_request",
                format!("sheet index {sheet} is out of range"),
            ));
        }
        if let Some(error) = scan_error {
            return Err(error);
        }

        if let Some(row) = pending_row {
            write_csv_row(&mut output, meta.cols, &pending_cells, cancel, out)?;
            next_output_row = row.saturating_add(1);
        }
        while next_output_row < meta.rows {
            write_csv_row(&mut output, meta.cols, &[], cancel, out)?;
            next_output_row = next_output_row.saturating_add(1);
        }

        output
            .flush()
            .map_err(|error| io_error("flush temporary CSV output", out, error))?;
    }
    temp.as_file()
        .sync_all()
        .map_err(|error| io_error("sync temporary CSV output", out, error))?;
    checkpoint(cancel)?;
    let bytes = temp
        .as_file()
        .metadata()
        .map_err(|error| io_error("inspect temporary CSV output", out, error))?
        .len();
    persist_output(temp, out)?;

    Ok(ExportOutcome {
        bytes,
        dropped: CSV_DROPPED
            .iter()
            .map(|entry| (*entry).to_owned())
            .collect(),
    })
}

#[derive(Default)]
struct Dropped {
    entries: Vec<String>,
    seen: HashSet<String>,
    omitted: usize,
}

impl Dropped {
    fn add(&mut self, entry: impl Into<String>) {
        let entry = entry.into();
        if self.seen.contains(&entry) {
            return;
        }
        if self.entries.len() < MAX_DROPPED_DETAILS {
            self.seen.insert(entry.clone());
            self.entries.push(entry);
        } else {
            self.omitted = self.omitted.saturating_add(1);
        }
    }

    fn finish(mut self) -> Vec<String> {
        if self.omitted != 0 {
            self.entries.push(format!(
                "{} additional dropped entries omitted",
                self.omitted
            ));
        }
        self.entries
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct FormatKey {
    num_format: Option<String>,
    style: Option<u32>,
}

#[derive(Clone, Debug)]
struct FormulaPatch {
    cell: String,
    formula: String,
    cached_result: Option<String>,
    cell_type: CellType,
}

fn normalize_formula_xml(
    source: NamedTempFile,
    out: &Path,
    sheets: &[Vec<FormulaPatch>],
    cancel: &AtomicBool,
) -> Result<NamedTempFile, WriteError> {
    if sheets.iter().all(Vec::is_empty) {
        return Ok(source);
    }

    let input = source
        .reopen()
        .map_err(|error| io_error("reopen temporary xlsx output", out, error))?;
    let mut input = ZipArchive::new(input)
        .map_err(|error| archive_error("open temporary xlsx archive", out, error))?;
    let mut normalized = temporary_output(out, ".xlsx")?;
    {
        let mut output = ZipWriter::new(normalized.as_file_mut());
        for index in 0..input.len() {
            checkpoint(cancel)?;
            let mut entry = input
                .by_index(index)
                .map_err(|error| archive_error("read temporary xlsx entry", out, error))?;
            let name = entry.name().to_owned();
            let mut options = SimpleFileOptions::default()
                .compression_method(entry.compression())
                .large_file(entry.size() > u64::from(u32::MAX));
            if let Some(modified) = entry.last_modified() {
                options = options.last_modified_time(modified);
            }
            if let Some(mode) = entry.unix_mode() {
                options = options.unix_permissions(mode);
            }

            if entry.is_dir() {
                output
                    .add_directory(&name, options)
                    .map_err(|error| archive_error("copy xlsx directory entry", out, error))?;
                continue;
            }
            output
                .start_file(&name, options)
                .map_err(|error| archive_error("create normalized xlsx entry", out, error))?;

            let patches = worksheet_patch_index(&name)
                .and_then(|sheet| sheets.get(sheet))
                .filter(|patches| !patches.is_empty());
            if let Some(patches) = patches {
                normalize_worksheet_formulas(&mut entry, &mut output, patches, out, cancel)?;
            } else {
                std::io::copy(&mut entry, &mut output)
                    .map_err(|error| io_error("copy temporary xlsx entry", out, error))?;
            }
        }
        output
            .finish()
            .map_err(|error| archive_error("finish normalized xlsx archive", out, error))?;
    }
    normalized
        .as_file()
        .sync_all()
        .map_err(|error| io_error("sync normalized xlsx output", out, error))?;
    Ok(normalized)
}

fn worksheet_patch_index(name: &str) -> Option<usize> {
    let number = name
        .strip_prefix("xl/worksheets/sheet")?
        .strip_suffix(".xml")?
        .parse::<usize>()
        .ok()?;
    number.checked_sub(1)
}

fn normalize_worksheet_formulas(
    input: &mut impl std::io::Read,
    output: &mut impl Write,
    patches: &[FormulaPatch],
    out: &Path,
    cancel: &AtomicBool,
) -> Result<(), WriteError> {
    let patch_by_cell = patches
        .iter()
        .enumerate()
        .map(|(index, patch)| (patch.cell.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut seen_formulas = HashSet::new();
    let mut reader = XmlReader::from_reader(BufReader::new(input));
    let mut writer = XmlWriter::new(output);
    let mut buffer = Vec::new();
    let mut current_patch = None;

    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| archive_error("parse generated worksheet XML", out, error))?;
        match event {
            Event::Start(start) if start.name().as_ref() == b"c" => {
                checkpoint(cancel)?;
                current_patch = start
                    .attributes()
                    .with_checks(false)
                    .filter_map(Result::ok)
                    .find(|attribute| attribute.key.as_ref() == b"r")
                    .and_then(|attribute| {
                        std::str::from_utf8(attribute.value.as_ref())
                            .ok()
                            .and_then(|cell| patch_by_cell.get(cell).copied())
                    });
                let start = if let Some(index) = current_patch {
                    normalized_formula_cell_start(start, &patches[index])
                } else {
                    start.into_owned()
                };
                writer
                    .write_event(Event::Start(start))
                    .map_err(|error| io_error("write normalized worksheet XML", out, error))?;
            }
            Event::End(end) if end.name().as_ref() == b"c" => {
                writer
                    .write_event(Event::End(end.into_owned()))
                    .map_err(|error| io_error("write normalized worksheet XML", out, error))?;
                current_patch = None;
            }
            Event::Start(start) if start.name().as_ref() == b"f" && current_patch.is_some() => {
                let patch_index = current_patch.expect("guarded by is_some");
                let patch = &patches[patch_index];
                seen_formulas.insert(patch_index);
                writer
                    .write_event(Event::Start(start.into_owned()))
                    .map_err(|error| io_error("write normalized worksheet XML", out, error))?;
                writer
                    .write_event(Event::Text(BytesText::new(&patch.formula)))
                    .map_err(|error| io_error("write normalized worksheet formula", out, error))?;
                skip_xml_element(&mut reader, b"f", &mut buffer, out)?;
                writer
                    .write_event(Event::End(BytesEnd::new("f")))
                    .map_err(|error| io_error("write normalized worksheet XML", out, error))?;
            }
            Event::Start(start) if start.name().as_ref() == b"v" && current_patch.is_some() => {
                let patch = &patches[current_patch.expect("guarded by is_some")];
                if let Some(result) = &patch.cached_result {
                    writer
                        .write_event(Event::Start(start.into_owned()))
                        .map_err(|error| io_error("write normalized worksheet XML", out, error))?;
                    writer
                        .write_event(Event::Text(BytesText::new(result)))
                        .map_err(|error| io_error("write normalized formula result", out, error))?;
                    skip_xml_element(&mut reader, b"v", &mut buffer, out)?;
                    writer
                        .write_event(Event::End(BytesEnd::new("v")))
                        .map_err(|error| io_error("write normalized worksheet XML", out, error))?;
                } else {
                    skip_xml_element(&mut reader, b"v", &mut buffer, out)?;
                }
            }
            Event::Eof => break,
            event => writer
                .write_event(event.into_owned())
                .map_err(|error| io_error("write normalized worksheet XML", out, error))?,
        }
        buffer.clear();
    }

    if seen_formulas.len() != patch_by_cell.len() {
        return Err(WriteError::new(
            "internal",
            format!(
                "generated worksheet contained {} of {} expected formulas",
                seen_formulas.len(),
                patch_by_cell.len()
            ),
        ));
    }
    Ok(())
}

fn normalized_formula_cell_start(
    start: BytesStart<'_>,
    patch: &FormulaPatch,
) -> BytesStart<'static> {
    let style = start
        .attributes()
        .with_checks(false)
        .filter_map(Result::ok)
        .find(|attribute| attribute.key.as_ref() == b"s")
        .and_then(|attribute| {
            std::str::from_utf8(attribute.value.as_ref())
                .ok()
                .map(str::to_owned)
        });
    let mut normalized = BytesStart::new("c");
    normalized.push_attribute(("r", patch.cell.as_str()));
    if let Some(style) = &style {
        normalized.push_attribute(("s", style.as_str()));
    }
    let result_type = if patch.cached_result.is_none() {
        Some("str")
    } else {
        match patch.cell_type {
            CellType::N | CellType::D => None,
            CellType::S => Some("str"),
            CellType::B => Some("b"),
            CellType::E => Some("e"),
        }
    };
    if let Some(result_type) = result_type {
        normalized.push_attribute(("t", result_type));
    }
    normalized.into_owned()
}

fn skip_xml_element(
    reader: &mut XmlReader<BufReader<&mut impl std::io::Read>>,
    name: &[u8],
    buffer: &mut Vec<u8>,
    out: &Path,
) -> Result<(), WriteError> {
    let mut depth = 1_u32;
    while depth > 0 {
        buffer.clear();
        let event = reader
            .read_event_into(buffer)
            .map_err(|error| archive_error("parse generated worksheet XML", out, error))?;
        match event {
            Event::Start(start) if start.name().as_ref() == name => depth += 1,
            Event::End(end) if end.name().as_ref() == name => depth -= 1,
            Event::Eof => {
                return Err(WriteError::new(
                    "internal",
                    "generated worksheet XML ended inside a formula value",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_xlsx_cell(
    worksheet: &mut Worksheet,
    formats: &mut HashMap<FormatKey, Format>,
    styles: &[CellStyle],
    sheet_name: &str,
    row: u32,
    column: u32,
    cell: &WindowCell,
    cached_formula_result: Option<&str>,
    dropped: &mut Dropped,
    out: &Path,
) -> Result<(), WriteError> {
    xlsx_row(row)?;
    let column = xlsx_column(column)?;
    let format_key = format_key(cell, styles, row, column)?;
    let format = intern_format(formats, styles, format_key)?;

    if let Some(formula_text) = &cell.f {
        let mut formula = Formula::new(formula_text);
        if let Some(result) = cached_formula_result {
            formula = formula.set_result(result);
        }
        match format {
            Some(format) => worksheet
                .write_formula_with_format(row, column, formula, format)
                .map(|_| ())
                .map_err(|error| xlsx_error("write formula cell", out, error)),
            None => worksheet
                .write_formula(row, column, formula)
                .map(|_| ())
                .map_err(|error| xlsx_error("write formula cell", out, error)),
        }
    } else {
        write_xlsx_value(
            worksheet, sheet_name, row, column, cell, format, dropped, out,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn write_xlsx_value(
    worksheet: &mut Worksheet,
    sheet_name: &str,
    row: u32,
    column: u16,
    cell: &WindowCell,
    format: Option<&Format>,
    dropped: &mut Dropped,
    out: &Path,
) -> Result<(), WriteError> {
    let result = match (&cell.t, &cell.v, format) {
        (_, None, Some(format)) => worksheet.write_blank(row, column, format),
        (_, None, None) => return Ok(()),
        (CellType::N, Some(CellValue::Number(value)), Some(format)) if value.is_finite() => {
            worksheet.write_number_with_format(row, column, *value, format)
        }
        (CellType::N, Some(CellValue::Number(value)), None) if value.is_finite() => {
            worksheet.write_number(row, column, *value)
        }
        // rust_xlsxwriter emits nothing for an empty string, so the cell
        // disappears from the exported sheet and cannot round-trip. The
        // readers do treat a stored empty string as a value (W5B restores
        // exactly these cells on the BIFF/XLSB paths), so this is a real
        // loss and the contract requires it to be loud rather than silent.
        // A formatted cell is still written blank so its styling survives.
        (CellType::S, Some(CellValue::Text(value)), format) if value.is_empty() => {
            dropped.add("empty strings are not representable in xlsx");
            match format {
                Some(format) => worksheet.write_blank(row, column, format),
                None => Ok(worksheet),
            }
        }
        (CellType::S, Some(CellValue::Text(value)), Some(format)) => {
            let value = truncate_xlsx_string(value, sheet_name, row, column, "string", dropped);
            worksheet.write_string_with_format(row, column, value, format)
        }
        (CellType::S, Some(CellValue::Text(value)), None) => {
            let value = truncate_xlsx_string(value, sheet_name, row, column, "string", dropped);
            worksheet.write_string(row, column, value)
        }
        (CellType::B, Some(CellValue::Bool(value)), Some(format)) => {
            worksheet.write_boolean_with_format(row, column, *value, format)
        }
        (CellType::B, Some(CellValue::Bool(value)), None) => {
            worksheet.write_boolean(row, column, *value)
        }
        (CellType::E, Some(CellValue::Text(value)), Some(format)) => {
            dropped.add("error cells written as text");
            let value = truncate_xlsx_string(value, sheet_name, row, column, "error text", dropped);
            worksheet.write_string_with_format(row, column, value, format)
        }
        (CellType::E, Some(CellValue::Text(value)), None) => {
            dropped.add("error cells written as text");
            let value = truncate_xlsx_string(value, sheet_name, row, column, "error text", dropped);
            worksheet.write_string(row, column, value)
        }
        (CellType::D, Some(CellValue::Text(value)), Some(format)) => {
            let datetime = parse_datetime(value, row, column)?;
            worksheet.write_datetime_with_format(row, column, &datetime, format)
        }
        (CellType::N, Some(CellValue::Number(value)), _) => {
            return Err(WriteError::new(
                "internal",
                format!(
                    "cell {} has non-finite number {value}",
                    cell_reference(row, column)
                ),
            ));
        }
        (cell_type, Some(value), _) => {
            return Err(type_mismatch(row, column, *cell_type, value));
        }
    };
    result
        .map(|_| ())
        .map_err(|error| xlsx_error("write cell", out, error))
}

fn format_key(
    cell: &WindowCell,
    styles: &[CellStyle],
    row: u32,
    column: u16,
) -> Result<Option<FormatKey>, WriteError> {
    let num_format = cell.fmt.clone().or_else(|| {
        (cell.t == CellType::D).then(|| default_date_format(cell.v.as_ref()).to_owned())
    });
    if let Some(style) = cell.s {
        if styles.get(style as usize).is_none() {
            return Err(WriteError::new(
                "internal",
                format!(
                    "cell {} references missing style {style}",
                    cell_reference(row, column)
                ),
            ));
        }
    }
    if num_format.is_none() && cell.s.is_none() {
        Ok(None)
    } else {
        Ok(Some(FormatKey {
            num_format,
            style: cell.s,
        }))
    }
}

fn intern_format<'a>(
    formats: &'a mut HashMap<FormatKey, Format>,
    styles: &[CellStyle],
    key: Option<FormatKey>,
) -> Result<Option<&'a Format>, WriteError> {
    let Some(key) = key else {
        return Ok(None);
    };
    if !formats.contains_key(&key) {
        let format = build_format(&key, styles)?;
        formats.insert(key.clone(), format);
    }
    Ok(formats.get(&key))
}

fn build_format(key: &FormatKey, styles: &[CellStyle]) -> Result<Format, WriteError> {
    let mut format = Format::new();
    if let Some(num_format) = &key.num_format {
        format = format.set_num_format(num_format);
    }
    let Some(style_id) = key.style else {
        return Ok(format);
    };
    let style = styles.get(style_id as usize).ok_or_else(|| {
        WriteError::new("internal", format!("missing style table entry {style_id}"))
    })?;
    if style.bold {
        format = format.set_bold();
    }
    if style.italic {
        format = format.set_italic();
    }
    if style.underline {
        format = format.set_underline(FormatUnderline::Single);
    }
    if style.strike {
        format = format.set_font_strikethrough();
    }
    if let Some(font_size) = style.font_size {
        if !font_size.is_finite() || font_size <= 0.0 {
            return Err(WriteError::new(
                "internal",
                format!("invalid font size {font_size} in style {style_id}"),
            ));
        }
        format = format.set_font_size(font_size);
    }
    if let Some(font_name) = &style.font_name {
        format = format.set_font_name(font_name);
    }
    if let Some(font_color) = &style.font_color {
        format = format.set_font_color(parse_color(font_color, style_id)?);
    }
    if let Some(fill_color) = &style.fill_color {
        format = format.set_background_color(parse_color(fill_color, style_id)?);
    }
    Ok(format)
}

fn parse_color(value: &str, style_id: u32) -> Result<Color, WriteError> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(WriteError::new(
            "internal",
            format!("invalid RGB color {value:?} in style {style_id}"),
        ));
    }
    let rgb = u32::from_str_radix(hex, 16).map_err(|error| {
        WriteError::new(
            "internal",
            format!("invalid RGB color {value:?} in style {style_id}: {error}"),
        )
    })?;
    Ok(Color::RGB(rgb))
}

fn formula_result(
    cell: &WindowCell,
    sheet_name: &str,
    row: u32,
    column: u16,
    dropped: &mut Dropped,
) -> Result<Option<String>, WriteError> {
    match (&cell.t, &cell.v) {
        (_, None) => Ok(None),
        (CellType::N, Some(CellValue::Number(value))) if value.is_finite() => {
            Ok(Some(value.to_string()))
        }
        (CellType::S | CellType::E, Some(CellValue::Text(value))) => Ok(Some(
            truncate_xlsx_string(
                value,
                sheet_name,
                row,
                column,
                "cached formula string",
                dropped,
            )
            .to_owned(),
        )),
        (CellType::B, Some(CellValue::Bool(true))) => Ok(Some("1".to_owned())),
        (CellType::B, Some(CellValue::Bool(false))) => Ok(Some("0".to_owned())),
        (CellType::D, Some(CellValue::Text(value))) => Ok(Some(
            parse_datetime(value, row, column)?.to_excel().to_string(),
        )),
        (CellType::N, Some(CellValue::Number(value))) => Err(WriteError::new(
            "internal",
            format!(
                "cell {} has non-finite formula result {value}",
                cell_reference(row, column)
            ),
        )),
        (cell_type, Some(value)) => Err(type_mismatch(row, column, *cell_type, value)),
    }
}

fn truncate_xlsx_string<'a>(
    value: &'a str,
    sheet_name: &str,
    row: u32,
    column: u16,
    kind: &str,
    dropped: &mut Dropped,
) -> &'a str {
    let Some((cut, _)) = value.char_indices().nth(XLSX_MAX_STRING_CHARS) else {
        return value;
    };
    let original_chars = XLSX_MAX_STRING_CHARS + value[cut..].chars().count();
    dropped.add(format!(
        "cell {} {kind} truncated from {original_chars} to {XLSX_MAX_STRING_CHARS} characters",
        qualified_cell_reference(sheet_name, row, column)
    ));
    &value[..cut]
}

fn qualified_cell_reference(sheet_name: &str, row: u32, column: u16) -> String {
    format!(
        "'{}'!{}",
        sheet_name.replace('\'', "''"),
        cell_reference(row, column)
    )
}

fn parse_datetime(value: &str, row: u32, column: u16) -> Result<ExcelDateTime, WriteError> {
    if has_timezone_offset(value) {
        return Err(WriteError::new(
            "internal",
            format!(
                "timezone offsets are unsupported in ISO-8601 date {value:?} at {}",
                cell_reference(row, column)
            ),
        ));
    }
    ExcelDateTime::parse_from_str(value).map_err(|error| {
        WriteError::new(
            "internal",
            format!(
                "invalid ISO-8601 date {value:?} at {}: {error}",
                cell_reference(row, column)
            ),
        )
    })
}

fn has_timezone_offset(value: &str) -> bool {
    let time = value
        .find(['T', ' '])
        .map_or(value, |separator| &value[separator + 1..]);
    time.contains(':') && time.contains(['+', '-'])
}

fn default_date_format(value: Option<&CellValue>) -> &'static str {
    let Some(CellValue::Text(value)) = value else {
        return DEFAULT_DATETIME_MILLIS_FORMAT;
    };
    let has_date = value.contains('-');
    let has_time = value.contains(':');
    let has_millis = value
        .split_once('.')
        .is_some_and(|(_, fraction)| !fraction.is_empty());
    match (has_date, has_time, has_millis) {
        (true, false, _) => DEFAULT_DATE_FORMAT,
        (true, true, true) => DEFAULT_DATETIME_MILLIS_FORMAT,
        (true, true, false) => DEFAULT_DATETIME_FORMAT,
        (false, true, true) => DEFAULT_TIME_MILLIS_FORMAT,
        (false, true, false) => DEFAULT_TIME_FORMAT,
        _ => DEFAULT_DATETIME_MILLIS_FORMAT,
    }
}

fn type_mismatch(row: u32, column: u16, cell_type: CellType, value: &CellValue) -> WriteError {
    WriteError::new(
        "internal",
        format!(
            "cell {} has type {cell_type:?} but incompatible value {value:?}",
            cell_reference(row, column)
        ),
    )
}

fn cell_reference(row: u32, column: u16) -> String {
    let mut number = u32::from(column) + 1;
    let mut letters = Vec::new();
    while number > 0 {
        letters.push((b'A' + ((number - 1) % 26) as u8) as char);
        number = (number - 1) / 26;
    }
    letters.reverse();
    format!(
        "{}{}",
        letters.into_iter().collect::<String>(),
        row.saturating_add(1)
    )
}

fn unique_xlsx_sheet_name(original: &str, used: &mut HashSet<String>) -> String {
    let mut characters = original
        .chars()
        .map(|character| {
            if matches!(character, '[' | ']' | ':' | '*' | '?' | '/' | '\\') {
                '_'
            } else {
                character
            }
        })
        .collect::<Vec<_>>();
    if characters.first() == Some(&'\'') {
        characters[0] = '_';
    }
    if characters.last() == Some(&'\'') {
        let last = characters.len() - 1;
        characters[last] = '_';
    }

    let mut base = characters.into_iter().take(31).collect::<String>();
    if base.is_empty() {
        base = "Sheet".to_owned();
    }

    let mut candidate = base.clone();
    let mut ordinal = 2_u32;
    while used.contains(&candidate.to_lowercase()) {
        let suffix = format!(" ({ordinal})");
        let prefix_len = 31_usize.saturating_sub(suffix.chars().count());
        candidate = base.chars().take(prefix_len).collect::<String>() + &suffix;
        ordinal = ordinal.saturating_add(1);
    }
    used.insert(candidate.to_lowercase());
    candidate
}

fn clamp_column_width(width: f64) -> f64 {
    if width.is_nan() {
        0.0
    } else {
        width.clamp(0.0, 255.0)
    }
}

fn parse_a1_range(range: &str) -> Option<(u32, u16, u32, u16)> {
    let (start, end) = range.split_once(':').unwrap_or((range, range));
    let (start_row, start_column) = parse_a1_cell(start)?;
    let (end_row, end_column) = parse_a1_cell(end)?;
    Some((
        start_row.min(end_row),
        start_column.min(end_column),
        start_row.max(end_row),
        start_column.max(end_column),
    ))
}

fn parse_a1_cell(reference: &str) -> Option<(u32, u16)> {
    let split = reference.find(|character: char| character.is_ascii_digit())?;
    let (letters, digits) = reference.split_at(split);
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let mut column = 0_u32;
    for letter in letters.chars() {
        if !letter.is_ascii_alphabetic() {
            return None;
        }
        let value = u32::from(letter.to_ascii_uppercase()) - u32::from('A') + 1;
        column = column.checked_mul(26)?.checked_add(value)?;
    }
    let row = digits.parse::<u32>().ok()?;
    if row == 0 || column == 0 {
        return None;
    }
    Some((row - 1, xlsx_column(column - 1).ok()?))
}

fn xlsx_column(column: u32) -> Result<u16, WriteError> {
    let column = u16::try_from(column).map_err(|_| {
        WriteError::new(
            "internal",
            format!("column index {column} exceeds the xlsx limit"),
        )
    })?;
    if column >= 16_384 {
        return Err(WriteError::new(
            "internal",
            format!("column index {column} exceeds the xlsx limit"),
        ));
    }
    Ok(column)
}

fn xlsx_row(row: u32) -> Result<u32, WriteError> {
    if row >= 1_048_576 {
        return Err(WriteError::new(
            "internal",
            format!("row index {row} exceeds the xlsx limit"),
        ));
    }
    Ok(row)
}

fn write_csv_row(
    output: &mut impl Write,
    columns: u32,
    cells: &[(u32, WindowCell)],
    cancel: &AtomicBool,
    out: &Path,
) -> Result<(), WriteError> {
    checkpoint(cancel)?;
    let mut cell_index = 0_usize;
    for column in 0..columns {
        if column > 0 {
            output
                .write_all(b",")
                .map_err(|error| io_error("write CSV output", out, error))?;
        }
        if let Some((cell_column, cell)) = cells.get(cell_index) {
            if *cell_column == column {
                write_csv_field(output, cell)
                    .map_err(|error| io_error("write CSV output", out, error))?;
                cell_index += 1;
            }
        }
    }
    output
        .write_all(b"\r\n")
        .map_err(|error| io_error("write CSV output", out, error))
}

fn write_csv_field(output: &mut impl Write, cell: &WindowCell) -> std::io::Result<()> {
    let raw;
    let value = if let Some(display) = &cell.d {
        display.as_str()
    } else {
        raw = match &cell.v {
            Some(CellValue::Number(value)) => value.to_string(),
            Some(CellValue::Text(value)) => value.clone(),
            Some(CellValue::Bool(true)) => "TRUE".to_owned(),
            Some(CellValue::Bool(false)) => "FALSE".to_owned(),
            None => String::new(),
        };
        &raw
    };
    if value.contains([',', '"', '\r', '\n']) {
        output.write_all(b"\"")?;
        for byte in value.bytes() {
            if byte == b'"' {
                output.write_all(b"\"\"")?;
            } else {
                output.write_all(&[byte])?;
            }
        }
        output.write_all(b"\"")
    } else {
        output.write_all(value.as_bytes())
    }
}

fn checkpoint(cancel: &AtomicBool) -> Result<(), WriteError> {
    if cancel.load(Ordering::Acquire) {
        Err(WriteError::new("cancelled", "export cancelled"))
    } else {
        Ok(())
    }
}

fn temporary_output(out: &Path, suffix: &str) -> Result<NamedTempFile, WriteError> {
    TempFileBuilder::new()
        .prefix(".wax-export-")
        .suffix(suffix)
        .tempfile_in(output_parent(out))
        .map_err(|error| io_error("create temporary output", out, error))
}

fn output_parent(out: &Path) -> &Path {
    out.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn persist_output(temp: NamedTempFile, out: &Path) -> Result<(), WriteError> {
    #[cfg(unix)]
    let create_permissions = file_create_permissions(out)?;

    let persisted = temp
        .persist(out)
        .map_err(|error| io_error("replace output", out, error.error))?;

    #[cfg(unix)]
    persisted
        .set_permissions(create_permissions)
        .map_err(|error| io_error("set output permissions", out, error))?;
    #[cfg(not(unix))]
    drop(persisted);

    Ok(())
}

#[cfg(unix)]
fn file_create_permissions(out: &Path) -> Result<std::fs::Permissions, WriteError> {
    use std::os::unix::fs::PermissionsExt;

    let mut builder = TempFileBuilder::new();
    builder
        .prefix(".wax-mode-")
        .permissions(std::fs::Permissions::from_mode(0o666));
    let probe = builder
        .tempfile_in(output_parent(out))
        .map_err(|error| io_error("determine output permissions", out, error))?;
    let mode = probe
        .as_file()
        .metadata()
        .map_err(|error| io_error("inspect output permissions", out, error))?
        .permissions()
        .mode()
        & 0o777;
    Ok(std::fs::Permissions::from_mode(mode))
}

fn io_error(action: &str, out: &Path, error: std::io::Error) -> WriteError {
    WriteError::new(
        "internal",
        format!("{action} for {} failed: {error}", out.display()),
    )
}

fn xlsx_error(action: &str, out: &Path, error: XlsxError) -> WriteError {
    WriteError::new(
        "internal",
        format!("{action} for {} failed: {error}", out.display()),
    )
}

fn archive_error(action: &str, out: &Path, error: impl std::fmt::Display) -> WriteError {
    WriteError::new(
        "internal",
        format!("{action} for {} failed: {error}", out.display()),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::Read;

    use super::*;
    use wax_core::{Cell, ColInfo, Document, Sheet};
    use wax_read::{CalamineReader, Reader, ReaderOptions};
    use zip::ZipArchive;

    #[allow(clippy::too_many_arguments)]
    fn cell(
        r: u32,
        c: u32,
        t: CellType,
        v: Option<CellValue>,
        d: Option<&str>,
        f: Option<&str>,
        fmt: Option<&str>,
        s: Option<u32>,
    ) -> Cell {
        Cell {
            r,
            c,
            t,
            v,
            d: d.map(str::to_owned),
            f: f.map(str::to_owned),
            fmt: fmt.map(str::to_owned),
            s,
        }
    }

    fn sheet(name: &str, rows: u32, cols: u32, cells: Vec<Cell>, merges: &[&str]) -> Sheet {
        Sheet {
            name: name.to_owned(),
            index: 0,
            rows,
            cols,
            truncated: false,
            merges: merges.iter().map(|range| (*range).to_owned()).collect(),
            cells,
            col_infos: Vec::new(),
        }
    }

    fn store_with(sheets: Vec<Sheet>, styles: Vec<CellStyle>) -> WorkbookStore {
        let mut document = Document::success("0.1.0", "model.xlsx", sheets, Vec::new());
        document.styles = styles;
        WorkbookStore::from_document(document)
    }

    fn read_xlsx(path: &Path) -> Document {
        let document = CalamineReader.read(
            path,
            ReaderOptions {
                max_cells: 100_000,
                ..ReaderOptions::default()
            },
        );
        assert!(
            document.ok,
            "read-back failed for {}: {:?}",
            path.display(),
            document.error
        );
        document
    }

    fn zip_text(path: &Path, name: &str) -> String {
        let file = File::open(path).expect("xlsx should open as a zip");
        let mut archive = ZipArchive::new(file).expect("xlsx should be a valid zip");
        let mut entry = archive.by_name(name).expect("zip entry should exist");
        let mut text = String::new();
        entry
            .read_to_string(&mut text)
            .expect("zip entry should be UTF-8");
        text
    }

    fn cells_by_position(document: &Document, sheet_index: usize) -> HashMap<(u32, u32), &Cell> {
        document.sheets[sheet_index]
            .cells
            .iter()
            .map(|cell| ((cell.r, cell.c), cell))
            .collect()
    }

    #[test]
    fn xlsx_maps_each_cell_type_and_reports_error_text_conversion() {
        let store = store_with(
            vec![sheet(
                "Types",
                1,
                5,
                vec![
                    cell(
                        0,
                        0,
                        CellType::N,
                        Some(CellValue::Number(1.25)),
                        None,
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        1,
                        CellType::S,
                        Some(CellValue::Text("hello".to_owned())),
                        None,
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        2,
                        CellType::B,
                        Some(CellValue::Bool(true)),
                        None,
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        3,
                        CellType::E,
                        Some(CellValue::Text("#DIV/0!".to_owned())),
                        None,
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        4,
                        CellType::D,
                        Some(CellValue::Text("2026-07-28".to_owned())),
                        None,
                        None,
                        None,
                        None,
                    ),
                ],
                &[],
            )],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("types.xlsx");

        let outcome = write_xlsx(&store, &out, &AtomicBool::new(false)).expect("xlsx should write");
        assert_eq!(outcome.bytes, std::fs::metadata(&out).unwrap().len());
        assert_eq!(outcome.dropped, ["error cells written as text"]);

        let actual = read_xlsx(&out);
        let cells = cells_by_position(&actual, 0);
        assert_eq!(cells[&(0, 0)].t, CellType::N);
        assert_eq!(cells[&(0, 0)].v, Some(CellValue::Number(1.25)));
        assert_eq!(cells[&(0, 1)].t, CellType::S);
        assert_eq!(cells[&(0, 1)].v, Some(CellValue::Text("hello".to_owned())));
        assert_eq!(cells[&(0, 2)].t, CellType::B);
        assert_eq!(cells[&(0, 2)].v, Some(CellValue::Bool(true)));
        assert_eq!(cells[&(0, 3)].t, CellType::S);
        assert_eq!(
            cells[&(0, 3)].v,
            Some(CellValue::Text("#DIV/0!".to_owned()))
        );
        assert_eq!(cells[&(0, 4)].t, CellType::D);
        assert_eq!(
            cells[&(0, 4)].v,
            Some(CellValue::Text("2026-07-28".to_owned()))
        );
    }

    #[test]
    fn xlsx_dates_round_trip_across_1900_epoch_times_and_milliseconds() {
        let values = [
            "1899-12-31",
            "1900-02-28",
            "1900-02-29",
            "1900-03-01",
            "2026-07-28T13:45:09",
            "2026-07-28T13:45:09.123",
        ];
        let cells = values
            .iter()
            .enumerate()
            .map(|(column, value)| {
                cell(
                    0,
                    column as u32,
                    CellType::D,
                    Some(CellValue::Text((*value).to_owned())),
                    None,
                    None,
                    None,
                    None,
                )
            })
            .collect();
        let store = store_with(
            vec![sheet("Dates", 1, values.len() as u32, cells, &[])],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("dates.xlsx");

        write_xlsx(&store, &out, &AtomicBool::new(false)).expect("xlsx should write");
        let actual = read_xlsx(&out);
        let actual = cells_by_position(&actual, 0);
        for (column, value) in values.iter().enumerate() {
            assert_eq!(actual[&(0, column as u32)].t, CellType::D);
            assert_eq!(
                actual[&(0, column as u32)].v,
                Some(CellValue::Text((*value).to_owned()))
            );
        }
    }

    #[test]
    fn xlsx_rejects_timezone_offsets_loudly_before_datetime_parsing() {
        let store = store_with(
            vec![sheet(
                "Dates",
                3,
                4,
                vec![cell(
                    2,
                    3,
                    CellType::D,
                    Some(CellValue::Text("2026-07-28T13:45:09+02:00".to_owned())),
                    None,
                    None,
                    None,
                    None,
                )],
                &[],
            )],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("offset.xlsx");

        let error = write_xlsx(&store, &out, &AtomicBool::new(false)).unwrap_err();
        assert_eq!(error.code, "internal");
        assert!(error.msg.contains("timezone offsets are unsupported"));
        assert!(error.msg.contains("D3"));
        assert!(!out.exists());
        assert!(!has_timezone_offset("2026-07-28T13:45:09Z"));
        assert!(parse_datetime("2026-07-28T13:45:09Z", 0, 0).is_ok());
    }

    #[test]
    fn xlsx_formulas_keep_text_and_typed_cached_results() {
        let expected = [
            (CellType::N, Some(CellValue::Number(3.5)), "1+2.5"),
            (
                CellType::S,
                Some(CellValue::Text("cached".to_owned())),
                r#""cached""#,
            ),
            (
                CellType::S,
                Some(CellValue::Text("123".to_owned())),
                r#""123""#,
            ),
            (
                CellType::S,
                Some(CellValue::Text("TRUE".to_owned())),
                r#""TRUE""#,
            ),
            (CellType::B, Some(CellValue::Bool(false)), "1=2"),
            (
                CellType::E,
                Some(CellValue::Text("#N/A".to_owned())),
                "NA()",
            ),
            (
                CellType::D,
                Some(CellValue::Text("2026-07-28T13:45:09.123".to_owned())),
                "NOW()",
            ),
            (CellType::N, None, "A99"),
        ];
        let cells = expected
            .iter()
            .enumerate()
            .map(|(column, (cell_type, value, formula))| {
                cell(
                    0,
                    column as u32,
                    *cell_type,
                    value.clone(),
                    None,
                    Some(formula),
                    None,
                    None,
                )
            })
            .collect();
        let store = store_with(
            vec![sheet("Formulas", 1, expected.len() as u32, cells, &[])],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("formulas.xlsx");

        let outcome = write_xlsx(&store, &out, &AtomicBool::new(false)).expect("xlsx should write");
        assert!(outcome.dropped.is_empty());
        let actual = read_xlsx(&out);
        let actual = cells_by_position(&actual, 0);
        for (column, (cell_type, value, formula)) in expected.iter().enumerate() {
            let actual = actual[&(0, column as u32)];
            // OOXML has no cached-result type marker when the result is
            // absent. The normalized contract permits the declared type or
            // string in that case; rust_xlsxwriter emits the latter.
            let expected_type = if value.is_none() {
                CellType::S
            } else {
                *cell_type
            };
            assert_eq!(actual.t, expected_type);
            assert_eq!(&actual.v, value);
            assert_eq!(actual.f.as_deref(), Some(*formula));
        }
    }

    #[test]
    fn xlsx_truncates_oversized_strings_on_character_boundaries_with_loud_drops() {
        let exact = "a".repeat(XLSX_MAX_STRING_CHARS);
        let oversized = "b".repeat(XLSX_MAX_STRING_CHARS + 1);
        let multibyte = format!("{}éTAIL", "c".repeat(XLSX_MAX_STRING_CHARS - 1));
        let cached_formula = "🙂".repeat(XLSX_MAX_STRING_CHARS + 1);
        let store = store_with(
            vec![sheet(
                "Strings",
                1,
                4,
                vec![
                    cell(
                        0,
                        0,
                        CellType::S,
                        Some(CellValue::Text(exact.clone())),
                        None,
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        1,
                        CellType::S,
                        Some(CellValue::Text(oversized)),
                        None,
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        2,
                        CellType::S,
                        Some(CellValue::Text(multibyte)),
                        None,
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        3,
                        CellType::S,
                        Some(CellValue::Text(cached_formula)),
                        None,
                        Some(r#""cached""#),
                        None,
                        None,
                    ),
                ],
                &[],
            )],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("long-strings.xlsx");

        let outcome = write_xlsx(&store, &out, &AtomicBool::new(false))
            .expect("oversized strings should truncate without failing the export");

        assert_eq!(
            outcome.dropped,
            [
                "cell 'Strings'!B1 string truncated from 32768 to 32767 characters",
                "cell 'Strings'!C1 string truncated from 32771 to 32767 characters",
                "cell 'Strings'!D1 cached formula string truncated from 32768 to 32767 characters",
            ]
        );
        let actual = read_xlsx(&out);
        let actual = cells_by_position(&actual, 0);
        assert_eq!(actual[&(0, 0)].v, Some(CellValue::Text(exact)));
        assert_eq!(
            actual[&(0, 1)].v,
            Some(CellValue::Text("b".repeat(XLSX_MAX_STRING_CHARS)))
        );
        assert_eq!(
            actual[&(0, 2)].v,
            Some(CellValue::Text(format!(
                "{}é",
                "c".repeat(XLSX_MAX_STRING_CHARS - 1)
            )))
        );
        assert_eq!(
            actual[&(0, 3)].v,
            Some(CellValue::Text("🙂".repeat(XLSX_MAX_STRING_CHARS)))
        );
        assert_eq!(actual[&(0, 3)].f.as_deref(), Some(r#""cached""#));
    }

    #[test]
    fn dropped_details_are_deduplicated_bounded_and_count_overflow() {
        let mut dropped = Dropped::default();
        for index in 0..MAX_DROPPED_DETAILS + 3 {
            dropped.add(format!("drop {index}"));
        }
        dropped.add("drop 0");

        let entries = dropped.finish();

        assert_eq!(entries.len(), MAX_DROPPED_DETAILS + 1);
        assert_eq!(entries.first().map(String::as_str), Some("drop 0"));
        assert_eq!(
            entries.last().map(String::as_str),
            Some("3 additional dropped entries omitted")
        );
    }

    #[test]
    fn truncation_drops_keep_same_cell_reference_on_different_sheets_distinct() {
        let value = "x".repeat(XLSX_MAX_STRING_CHARS + 1);
        let mut dropped = Dropped::default();

        truncate_xlsx_string(&value, "First", 0, 0, "string", &mut dropped);
        truncate_xlsx_string(&value, "Second", 0, 0, "string", &mut dropped);

        assert_eq!(
            dropped.finish(),
            [
                "cell 'First'!A1 string truncated from 32768 to 32767 characters",
                "cell 'Second'!A1 string truncated from 32768 to 32767 characters",
            ]
        );
    }

    #[test]
    fn xlsx_deduplicates_format_pairs_and_maps_basic_styles() {
        let style = CellStyle {
            bold: true,
            italic: true,
            underline: true,
            strike: true,
            font_size: Some(13.5),
            font_name: Some("Aptos".to_owned()),
            font_color: Some("#112233".to_owned()),
            fill_color: Some("#AABBCC".to_owned()),
        };
        let repeated_format = r#"0.000 "wax""#;
        let cells = (0..3)
            .map(|column| {
                cell(
                    0,
                    column,
                    CellType::N,
                    Some(CellValue::Number(f64::from(column))),
                    None,
                    None,
                    Some(repeated_format),
                    Some(0),
                )
            })
            .chain(std::iter::once(cell(
                0,
                3,
                CellType::N,
                Some(CellValue::Number(3.0)),
                None,
                None,
                Some(r#"0.0 "other""#),
                Some(0),
            )))
            .collect();
        let store = store_with(vec![sheet("Styles", 1, 4, cells, &[])], vec![style]);
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("styles.xlsx");

        write_xlsx(&store, &out, &AtomicBool::new(false)).expect("xlsx should write");
        let styles = zip_text(&out, "xl/styles.xml");
        assert!(styles.contains(r#"<numFmts count="2">"#), "{styles}");
        // Default + one repeated pair + one different number format.
        assert!(styles.contains(r#"<cellXfs count="3">"#), "{styles}");
        assert!(styles.contains("<b/>"), "{styles}");
        assert!(styles.contains("<i/>"), "{styles}");
        assert!(styles.contains("<u/>"), "{styles}");
        assert!(styles.contains("<strike/>"), "{styles}");
        assert!(styles.contains(r#"<sz val="13.5"/>"#), "{styles}");
        assert!(styles.contains(r#"<name val="Aptos"/>"#), "{styles}");
        assert!(styles.contains(r#"rgb="FF112233""#), "{styles}");
        assert!(styles.contains(r#"rgb="FFAABBCC""#), "{styles}");
    }

    #[test]
    fn xlsx_preserves_value_and_blank_anchor_merges() {
        let store = store_with(
            vec![sheet(
                "Merges",
                4,
                3,
                vec![cell(
                    0,
                    0,
                    CellType::S,
                    Some(CellValue::Text("anchor".to_owned())),
                    None,
                    None,
                    None,
                    None,
                )],
                &["A1:B1", "A3:C4"],
            )],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("merges.xlsx");

        write_xlsx(&store, &out, &AtomicBool::new(false)).expect("xlsx should write");
        let actual = read_xlsx(&out);
        assert_eq!(actual.sheets[0].merges, ["A1:B1", "A3:C4"]);
        let cells = cells_by_position(&actual, 0);
        assert_eq!(cells[&(0, 0)].v, Some(CellValue::Text("anchor".to_owned())));
        assert!(!cells.contains_key(&(2, 0)));
    }

    #[test]
    fn xlsx_skips_single_overlapping_and_unparseable_merge_ranges_loudly() {
        let store = store_with(
            vec![sheet(
                "Merges",
                3,
                4,
                Vec::new(),
                &["A1:B2", "B2:C3", "D1", "XFE1:XFE2"],
            )],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("unrepresentable-merges.xlsx");

        let outcome = write_xlsx(&store, &out, &AtomicBool::new(false))
            .expect("unrepresentable merges should not fail the export");
        assert_eq!(outcome.dropped, [UNREPRESENTABLE_MERGES_DROPPED]);
        assert_eq!(read_xlsx(&out).sheets[0].merges, ["A1:B2"]);
    }

    #[test]
    fn xlsx_sanitizes_sheet_names_and_keeps_them_unique() {
        let long_a = "abcdefghijklmnopqrstuvwxyz12345-A";
        let long_b = "abcdefghijklmnopqrstuvwxyz12345-B";
        let original_names = ["Normal", "bad[]:*?/\\name", long_a, long_b];
        let store = store_with(
            original_names
                .iter()
                .map(|name| sheet(name, 0, 0, Vec::new(), &[]))
                .collect(),
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("sheet-names.xlsx");

        let outcome = write_xlsx(&store, &out, &AtomicBool::new(false))
            .expect("invalid source sheet names should be sanitized");
        let actual = read_xlsx(&out);
        let actual_names = actual
            .sheets
            .iter()
            .map(|sheet| sheet.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(actual_names[0], "Normal");
        assert_eq!(actual_names[1], "bad_______name");
        assert_eq!(actual_names[2], "abcdefghijklmnopqrstuvwxyz12345");
        assert!(actual_names[3].ends_with(" (2)"));
        assert!(actual_names.iter().all(|name| name.chars().count() <= 31));
        assert_eq!(
            actual_names
                .iter()
                .map(|name| name.to_lowercase())
                .collect::<HashSet<_>>()
                .len(),
            original_names.len()
        );
        assert_eq!(outcome.dropped.len(), 3);
        for original in &original_names[1..] {
            assert!(
                outcome.dropped.iter().any(|entry| entry.contains(original)),
                "missing dropped entry naming {original:?}: {:?}",
                outcome.dropped
            );
        }
    }

    #[test]
    fn xlsx_writes_explicit_column_widths() {
        let mut model = sheet("Widths", 0, 3, Vec::new(), &[]);
        model.col_infos = vec![ColInfo { c: 0, width: 8.5 }, ColInfo { c: 2, width: 24.25 }];
        let store = store_with(vec![model], Vec::new());
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("widths.xlsx");

        write_xlsx(&store, &out, &AtomicBool::new(false)).expect("xlsx should write");
        let worksheet = zip_text(&out, "xl/worksheets/sheet1.xml");
        assert!(worksheet.contains(r#"<col min="1" max="1" width=""#));
        assert!(worksheet.contains(r#"<col min="3" max="3" width=""#));
        assert_eq!(worksheet.matches(r#"customWidth="1""#).count(), 2);
    }

    #[test]
    fn xlsx_clamps_out_of_range_column_widths_loudly() {
        let mut model = sheet("Widths", 0, 2, Vec::new(), &[]);
        model.col_infos = vec![
            ColInfo { c: 0, width: -12.0 },
            ColInfo { c: 1, width: 300.0 },
        ];
        let store = store_with(vec![model], Vec::new());
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("clamped-widths.xlsx");

        let outcome = write_xlsx(&store, &out, &AtomicBool::new(false))
            .expect("out-of-range widths should be clamped");
        assert_eq!(outcome.dropped, [CLAMPED_COLUMN_WIDTHS_DROPPED]);
        assert_eq!(clamp_column_width(-12.0), 0.0);
        assert_eq!(clamp_column_width(300.0), 255.0);
        assert!(out.is_file());
    }

    #[test]
    fn xlsx_dropped_entries_are_loud_deduplicated_and_deterministic() {
        let mut model = sheet(
            "Dropped",
            1,
            2,
            vec![
                cell(
                    0,
                    0,
                    CellType::E,
                    Some(CellValue::Text("#REF!".to_owned())),
                    None,
                    None,
                    None,
                    None,
                ),
                cell(
                    0,
                    1,
                    CellType::E,
                    Some(CellValue::Text("#N/A".to_owned())),
                    None,
                    None,
                    None,
                    None,
                ),
            ],
            &[],
        );
        model.truncated = true;
        let store = store_with(vec![model], Vec::new());
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("dropped.xlsx");

        let outcome = write_xlsx(&store, &out, &AtomicBool::new(false)).expect("xlsx should write");
        assert_eq!(
            outcome.dropped,
            [
                "source truncated at read time; export is the truncated model",
                "error cells written as text"
            ]
        );
    }

    #[test]
    fn cancellation_leaves_no_new_xlsx_or_csv_output() {
        let store = store_with(vec![sheet("Sheet1", 1, 1, Vec::new(), &[])], Vec::new());
        let temp = tempfile::tempdir().expect("temporary directory");
        let xlsx = temp.path().join("cancelled.xlsx");
        let csv = temp.path().join("cancelled.csv");
        let cancel = AtomicBool::new(true);

        assert_eq!(
            write_xlsx(&store, &xlsx, &cancel).unwrap_err().code,
            "cancelled"
        );
        assert_eq!(
            write_csv(&store, 0, &csv, &cancel).unwrap_err().code,
            "cancelled"
        );
        assert!(!xlsx.exists());
        assert!(!csv.exists());
    }

    #[cfg(unix)]
    #[test]
    fn persisted_outputs_use_file_create_mode_for_new_and_replaced_files() {
        use std::os::unix::fs::PermissionsExt;

        let store = store_with(vec![sheet("Sheet1", 0, 0, Vec::new(), &[])], Vec::new());
        let temp = tempfile::tempdir().expect("temporary directory");
        let mode_probe = temp.path().join("file-create-mode");
        drop(File::create(&mode_probe).expect("mode probe should be created"));
        let expected_mode = mode_probe
            .metadata()
            .expect("mode probe metadata")
            .permissions()
            .mode()
            & 0o777;

        let xlsx = temp.path().join("new.xlsx");
        write_xlsx(&store, &xlsx, &AtomicBool::new(false)).expect("xlsx should write");

        let csv = temp.path().join("replaced.csv");
        std::fs::write(&csv, b"existing").expect("existing output should be created");
        std::fs::set_permissions(&csv, std::fs::Permissions::from_mode(0o600))
            .expect("existing mode should be restricted");
        write_csv(&store, 0, &csv, &AtomicBool::new(false)).expect("CSV should replace");

        for output in [&xlsx, &csv] {
            let actual_mode = output
                .metadata()
                .expect("output metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(
                actual_mode,
                expected_mode,
                "{} should use File::create mode",
                output.display()
            );
        }
    }

    #[test]
    fn xlsx_errors_do_not_replace_an_existing_output() {
        let store = store_with(
            vec![sheet(
                "Sheet1",
                1,
                1,
                vec![cell(
                    0,
                    0,
                    CellType::D,
                    Some(CellValue::Text("not-a-date".to_owned())),
                    None,
                    None,
                    None,
                    None,
                )],
                &[],
            )],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("existing.xlsx");
        std::fs::write(&out, b"original").unwrap();

        assert_eq!(
            write_xlsx(&store, &out, &AtomicBool::new(false))
                .unwrap_err()
                .code,
            "internal"
        );
        assert_eq!(std::fs::read(&out).unwrap(), b"original");
    }

    #[test]
    fn csv_matches_rfc_4180_and_protocol_value_spellings() {
        let store = store_with(
            vec![sheet(
                "CSV",
                2,
                6,
                vec![
                    cell(
                        0,
                        0,
                        CellType::S,
                        Some(CellValue::Text("raw".to_owned())),
                        Some("say \"hello\",\nfriend"),
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        1,
                        CellType::N,
                        Some(CellValue::Number(1.2345678901234567)),
                        None,
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        2,
                        CellType::B,
                        Some(CellValue::Bool(true)),
                        None,
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        3,
                        CellType::B,
                        Some(CellValue::Bool(false)),
                        None,
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        4,
                        CellType::E,
                        Some(CellValue::Text("#DIV/0!".to_owned())),
                        None,
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        5,
                        CellType::N,
                        Some(CellValue::Number(3.0)),
                        None,
                        Some("1+2"),
                        None,
                        None,
                    ),
                ],
                &[],
            )],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("values.csv");

        let outcome =
            write_csv(&store, 0, &out, &AtomicBool::new(false)).expect("CSV should write");
        let expected = concat!(
            "\"say \"\"hello\"\",\nfriend\",",
            "1.2345678901234567,TRUE,FALSE,#DIV/0!,3\r\n",
            ",,,,,\r\n"
        )
        .as_bytes();
        assert_eq!(std::fs::read(&out).unwrap(), expected);
        assert_eq!(outcome.bytes, expected.len() as u64);
        assert_eq!(
            outcome.dropped,
            [
                "formulas (cached values only)",
                "number formatting beyond display strings",
                "merges",
                "styles",
                "column widths",
            ]
        );
    }

    #[test]
    fn csv_preserves_strings_beyond_the_xlsx_character_limit() {
        let value = "é".repeat(XLSX_MAX_STRING_CHARS + 1);
        let store = store_with(
            vec![sheet(
                "CSV",
                1,
                1,
                vec![cell(
                    0,
                    0,
                    CellType::S,
                    Some(CellValue::Text(value.clone())),
                    None,
                    None,
                    None,
                    None,
                )],
                &[],
            )],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("long-string.csv");

        write_csv(&store, 0, &out, &AtomicBool::new(false))
            .expect("CSV should keep the full string");

        let mut expected = value.into_bytes();
        expected.extend_from_slice(b"\r\n");
        assert_eq!(std::fs::read(out).unwrap(), expected);
    }

    #[test]
    fn csv_uses_last_duplicate_cell_and_rejects_bad_sheet() {
        let duplicate_cells = vec![
            cell(
                0,
                0,
                CellType::S,
                Some(CellValue::Text("first".to_owned())),
                None,
                None,
                None,
                None,
            ),
            cell(
                0,
                0,
                CellType::S,
                Some(CellValue::Text("last".to_owned())),
                None,
                None,
                None,
                None,
            ),
        ];
        let store = store_with(
            vec![sheet("Sheet1", 1, 1, duplicate_cells, &[])],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("duplicate.csv");

        write_csv(&store, 0, &out, &AtomicBool::new(false)).expect("CSV should write");
        assert_eq!(std::fs::read(&out).unwrap(), b"last\r\n");
        assert_eq!(
            write_csv(
                &store,
                99,
                &temp.path().join("bad.csv"),
                &AtomicBool::new(false)
            )
            .unwrap_err()
            .code,
            "bad_request"
        );
    }

    #[test]
    fn xlsx_uses_last_duplicate_when_a_formula_is_overwritten() {
        let duplicate_cells = vec![
            cell(
                0,
                0,
                CellType::N,
                Some(CellValue::Number(2.0)),
                None,
                Some("1+1"),
                None,
                None,
            ),
            cell(
                0,
                0,
                CellType::S,
                Some(CellValue::Text("replacement".to_owned())),
                None,
                None,
                None,
                None,
            ),
        ];
        let store = store_with(
            vec![sheet("Sheet1", 1, 1, duplicate_cells, &[])],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("duplicate.xlsx");

        write_xlsx(&store, &out, &AtomicBool::new(false)).expect("xlsx should write");
        let actual = read_xlsx(&out);
        assert_eq!(actual.sheets[0].cells.len(), 1);
        assert_eq!(
            actual.sheets[0].cells[0].v,
            Some(CellValue::Text("replacement".to_owned()))
        );
        assert!(actual.sheets[0].cells[0].f.is_none());
    }

    #[test]
    fn xlsx_rejects_an_empty_workbook_without_creating_output() {
        let store = WorkbookStore::default();
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("empty.xlsx");

        let error = write_xlsx(&store, &out, &AtomicBool::new(false)).unwrap_err();
        assert_eq!(error, WriteError::new("bad_request", "empty workbook"));
        assert!(!out.exists());
    }

    #[test]
    fn committed_xlsx_fixture_round_trips_all_normalized_cells() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fixtures = [repository.join("crates/wax-read/tests/fixtures/reader.xlsx")];
        let temp = tempfile::tempdir().expect("temporary directory");

        for (index, fixture) in fixtures.iter().enumerate() {
            let out = temp.path().join(format!("fixture-{index}.xlsx"));
            let _ = assert_fixture_round_trip(fixture, &out);
        }
    }

    #[test]
    #[ignore = "requires the machine-local, gitignored corpus payload overlay"]
    fn five_local_corpus_fixtures_round_trip_all_normalized_cells() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let corpus_candidates = [manifest.join("../.."), manifest.join("../../../..")]
            .map(|repository| repository.join("corpus/files/poi/test-data/spreadsheet"));
        let corpus = corpus_candidates
            .iter()
            .find(|candidate| candidate.is_dir())
            .unwrap_or_else(|| {
                panic!(
                    "machine-local corpus overlay is missing; tried: {}",
                    corpus_candidates
                        .iter()
                        .map(|candidate| candidate.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            });
        let fixtures = [
            corpus.join("Booleans.xlsx"),
            corpus.join("53282b.xlsx"),
            corpus.join("unicodeSheetName.xlsx"),
            corpus.join("testSharedFormulasSetBlank.xlsx"),
            corpus.join("56011.xlsx"),
        ];
        let temp = tempfile::tempdir().expect("temporary directory");
        let mut exact = 0_usize;
        let mut total = 0_usize;

        for (index, fixture) in fixtures.iter().enumerate() {
            assert!(
                fixture.is_file(),
                "machine-local fixture is missing: {}",
                fixture.display()
            );
            let stats = assert_fixture_round_trip(
                fixture,
                &temp.path().join(format!("corpus-{index}.xlsx")),
            );
            exact += stats.0;
            total += stats.1;
        }
        eprintln!("local corpus round-trip exact (t, v): {exact}/{total}");
        assert!(exact * 100 >= total * 99, "{exact}/{total} is below 99%");
    }

    fn assert_fixture_round_trip(fixture: &Path, out: &Path) -> (usize, usize) {
        let source = read_xlsx(fixture);
        let store = WorkbookStore::from_document(source.clone());
        write_xlsx(&store, out, &AtomicBool::new(false))
            .unwrap_or_else(|error| panic!("{} should export: {error}", fixture.display()));
        let actual = read_xlsx(out);

        assert_eq!(actual.sheets.len(), source.sheets.len());
        let mut exact = 0_usize;
        let mut total = 0_usize;
        for (sheet_index, source_sheet) in source.sheets.iter().enumerate() {
            let actual_sheet = &actual.sheets[sheet_index];
            assert_eq!(actual_sheet.name, source_sheet.name);
            assert_eq!(actual_sheet.merges, source_sheet.merges);
            let actual_cells = cells_by_position(&actual, sheet_index);
            for source_cell in &source_sheet.cells {
                let actual_cell = actual_cells
                    .get(&(source_cell.r, source_cell.c))
                    .unwrap_or_else(|| {
                        panic!(
                            "{} sheet {sheet_index} lost cell ({}, {})",
                            fixture.display(),
                            source_cell.r,
                            source_cell.c
                        )
                    });
                let expected_type = if source_cell.t == CellType::E && source_cell.f.is_none() {
                    CellType::S
                } else if source_cell.v.is_none() {
                    // See the uncached formula note in the focused formula
                    // test above.
                    CellType::S
                } else {
                    source_cell.t
                };
                assert_eq!(actual_cell.t, expected_type);
                assert_eq!(actual_cell.v, source_cell.v);
                assert_eq!(actual_cell.f, source_cell.f);
                total += 1;
                if actual_cell.t == source_cell.t && actual_cell.v == source_cell.v {
                    exact += 1;
                }
            }
        }
        (exact, total)
    }

    #[test]
    fn structured_errors_keep_proto_code_spellings() {
        let store = store_with(
            vec![sheet(
                "Sheet1",
                1,
                1,
                vec![cell(
                    0,
                    0,
                    CellType::N,
                    Some(CellValue::Text("wrong".to_owned())),
                    None,
                    None,
                    None,
                    None,
                )],
                &[],
            )],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");

        let internal = write_xlsx(
            &store,
            &temp.path().join("bad.xlsx"),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(internal.code, "internal");
        assert!(internal.msg.contains("incompatible value"));

        let bad_request = write_csv(
            &store,
            9,
            &temp.path().join("bad.csv"),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(bad_request.code, "bad_request");

        let cancelled = write_csv(
            &store,
            0,
            &temp.path().join("cancelled.csv"),
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert_eq!(cancelled.code, "cancelled");
    }
}

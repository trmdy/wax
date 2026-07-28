use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::{Duration, Instant};

use calamine::{
    expand_shared_formula, open_workbook, Data, Dimensions, ExcelDateTime, ExcelDateTimeType, Ods,
    Range, Reader as CalamineWorkbook, SheetType, Xls, Xlsb, Xlsx, XlsxFormulaMetadata,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader as XmlReader;
use wax_core::{Cell, CellStyle, CellType, CellValue, ColInfo, Document, DumpError, Sheet};
use wax_fmt::{render, FmtValue};
use wax_proto::ErrorCode;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::{Reader, ReaderOptions};

#[path = "xls_styles.rs"]
mod xls_styles;
#[path = "xlsb_styles.rs"]
mod xlsb_styles;

/// Workbook reader backed by calamine, with wax's normalization layered on top.
#[derive(Clone, Copy, Debug, Default)]
pub struct CalamineReader;

impl Reader for CalamineReader {
    fn read(&self, path: &Path, options: ReaderOptions) -> Document {
        let file = path.to_string_lossy().into_owned();
        let result = catch_unwind(AssertUnwindSafe(|| read_workbook(path, options)));
        match result {
            Ok(Ok(outcome)) => {
                let mut document = Document::success(
                    env!("CARGO_PKG_VERSION"),
                    file,
                    outcome.sheets,
                    outcome.warnings,
                );
                document.styles = outcome.styles;
                document
            }
            Ok(Err(failure)) => failure_document(file, failure),
            Err(_) => failure_document(
                file,
                ReadFailure::new(
                    ErrorCode::Internal,
                    "calamine panicked while reading the workbook",
                ),
            ),
        }
    }
}

struct ReadOutcome {
    sheets: Vec<Sheet>,
    warnings: Vec<String>,
    styles: Vec<CellStyle>,
}

fn read_workbook(path: &Path, options: ReaderOptions) -> Result<ReadOutcome, ReadFailure> {
    let kind = WorkbookKind::from_path(path).ok_or_else(|| {
        ReadFailure::new(
            ErrorCode::Unsupported,
            "supported extensions are .xlsx, .xlsm, .xlsb, .xls, and .ods",
        )
    })?;
    let started = Instant::now();
    let timeout = Duration::from_millis(options.timeout_ms);
    ensure_time_remaining(started, timeout)?;

    match kind {
        WorkbookKind::Xlsx => read_xlsx(path, options.max_cells, started, timeout),
        WorkbookKind::Xlsb => {
            read_xlsb(path, options.max_cells, started, timeout).map(|(sheets, warnings)| {
                ReadOutcome {
                    sheets,
                    warnings,
                    styles: Vec::new(),
                }
            })
        }
        WorkbookKind::Xls => {
            read_xls(path, options.max_cells, started, timeout).map(|(sheets, warnings)| {
                ReadOutcome {
                    sheets,
                    warnings,
                    styles: Vec::new(),
                }
            })
        }
        WorkbookKind::Ods => {
            read_ods(path, options.max_cells, started, timeout).map(|(sheets, warnings)| {
                ReadOutcome {
                    sheets,
                    warnings,
                    styles: Vec::new(),
                }
            })
        }
    }
}

#[derive(Clone, Copy)]
enum WorkbookKind {
    Xlsx,
    Xlsb,
    Xls,
    Ods,
}

impl WorkbookKind {
    fn from_path(path: &Path) -> Option<Self> {
        match path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("xlsx" | "xlsm") => Some(Self::Xlsx),
            Some("xlsb") => Some(Self::Xlsb),
            Some("xls") => Some(Self::Xls),
            Some("ods") => Some(Self::Ods),
            _ => None,
        }
    }
}

fn read_xlsx(
    path: &Path,
    max_cells: usize,
    started: Instant,
    timeout: Duration,
) -> Result<ReadOutcome, ReadFailure> {
    if let Ok(Some(reader)) = normalize_nonstandard_workbook_package(path) {
        let workbook =
            Xlsx::new(reader).map_err(|error| container_error(WorkbookKind::Xlsx, error))?;
        return read_xlsx_workbook(workbook, path, max_cells, started, timeout, true);
    }
    let workbook: Xlsx<BufReader<File>> =
        open_workbook(path).map_err(|error| container_error(WorkbookKind::Xlsx, error))?;
    read_xlsx_workbook(workbook, path, max_cells, started, timeout, false)
}

fn read_xlsx_workbook<RS: Read + Seek>(
    mut workbook: Xlsx<RS>,
    path: &Path,
    max_cells: usize,
    started: Instant,
    timeout: Duration,
    normalized_workbook_part: bool,
) -> Result<ReadOutcome, ReadFailure> {
    let epoch_1904 = workbook.has_1904_epoch();
    let metadata = workbook.sheets_metadata().to_vec();
    let mut warnings = Vec::new();
    if normalized_workbook_part {
        warnings.push("nonstandard OOXML workbook part normalized in memory".to_owned());
    }
    let supplement = match OoxmlSupplement::read(path) {
        Ok(supplement) => supplement,
        Err(message) => {
            warnings.push(format!("xlsx metadata could not be read: {message}"));
            OoxmlSupplement::default()
        }
    };
    warnings.extend(supplement.warnings.iter().cloned());

    let mut emitted = 0_usize;
    let mut sheets = Vec::with_capacity(metadata.len());
    let mut styles = Vec::new();
    for (index, metadata) in metadata.iter().enumerate() {
        ensure_time_remaining(started, timeout)?;
        if metadata.typ != SheetType::WorkSheet {
            warnings.push(format!(
                "sheet {:?} is not a worksheet and was emitted empty",
                metadata.name
            ));
            sheets.push(empty_sheet(&metadata.name, index));
            continue;
        }

        let mut candidates = Vec::new();
        let dimensions;
        {
            let mut cells = workbook
                .worksheet_cells_reader(&metadata.name)
                .map_err(|error| container_error(WorkbookKind::Xlsx, error))?;
            dimensions = cells.dimensions();
            let mut shared_formulas = HashMap::<usize, ((u32, u32), String)>::new();
            let mut seen = 0_usize;
            while let Some(record) = cells
                .next_cell_with_formula_metadata()
                .map_err(|error| container_error(WorkbookKind::Xlsx, error))?
            {
                seen += 1;
                if seen.is_multiple_of(4096) {
                    ensure_time_remaining(started, timeout)?;
                }
                let cell_metadata = supplement.cell(&metadata.name, record.pos);
                let formula =
                    normalize_xlsx_formula(record.formula, record.pos, &mut shared_formulas);
                let value = Data::from(record.value);
                // Empty OOXML strings without a formula are layout/style
                // placeholders, not value-bearing cells in the normalized
                // sparse model. Formula cells remain present even when their
                // cached string is empty.
                if formula.is_none() && matches!(&value, Data::String(text) if text.is_empty()) {
                    continue;
                }
                if let Some(mut cell) = normalize_cell(
                    record.pos,
                    &value,
                    formula,
                    cell_metadata.and_then(|metadata| metadata.format),
                    cell_metadata.and_then(|metadata| metadata.declared_type),
                    epoch_1904,
                ) {
                    cell.s = cell_metadata
                        .and_then(|metadata| metadata.style)
                        .and_then(|style| intern_style(&mut styles, style));
                    candidates.push(cell);
                }
            }
        }

        let merges = workbook
            .merge_cells_by_sheet_name(&metadata.name)
            .map_err(|error| container_error(WorkbookKind::Xlsx, error))?;
        let (rows, cols) = extent_from_dimensions(dimensions, &candidates);
        let mut sheet = finish_sheet(
            SheetDraft {
                name: &metadata.name,
                index,
                rows,
                cols,
                merges,
                candidates,
            },
            &mut emitted,
            max_cells,
        );
        sheet.col_infos = supplement.col_infos(&metadata.name, cols);
        sheets.push(sheet);
    }
    let styles = compact_styles(&mut sheets, &styles);
    Ok(ReadOutcome {
        sheets,
        warnings,
        styles,
    })
}

fn normalize_xlsx_formula(
    metadata: Option<XlsxFormulaMetadata>,
    position: (u32, u32),
    shared_formulas: &mut HashMap<usize, ((u32, u32), String)>,
) -> Option<String> {
    match metadata? {
        XlsxFormulaMetadata::Normal { formula } => normalize_formula(formula),
        XlsxFormulaMetadata::Shared {
            shared_index,
            formula,
            ..
        } => {
            let formula = normalize_formula(formula)?;
            shared_formulas.insert(shared_index, (position, formula.clone()));
            Some(formula)
        }
        XlsxFormulaMetadata::SharedDerived { shared_index } => {
            let (anchor, formula) = shared_formulas.get(&shared_index)?;
            expand_shared_formula(formula, *anchor, position)
                .ok()
                .and_then(normalize_formula)
        }
        _ => None,
    }
}

fn read_xlsb(
    path: &Path,
    max_cells: usize,
    started: Instant,
    timeout: Duration,
) -> Result<(Vec<Sheet>, Vec<String>), ReadFailure> {
    if let Ok(Some(reader)) = xlsb_styles::normalize_legacy_bundle_workbook(path) {
        let workbook =
            Xlsb::new(reader).map_err(|error| container_error(WorkbookKind::Xlsb, error))?;
        return read_xlsb_workbook(workbook, path, max_cells, started, timeout, true);
    }
    let workbook: Xlsb<BufReader<File>> =
        open_workbook(path).map_err(|error| container_error(WorkbookKind::Xlsb, error))?;
    read_xlsb_workbook(workbook, path, max_cells, started, timeout, false)
}

fn read_xlsb_workbook<RS: Read + Seek>(
    mut workbook: Xlsb<RS>,
    path: &Path,
    max_cells: usize,
    started: Instant,
    timeout: Duration,
    normalized_legacy_bundle: bool,
) -> Result<(Vec<Sheet>, Vec<String>), ReadFailure> {
    let epoch_1904 = workbook.has_1904_epoch();
    let metadata = workbook.sheets_metadata().to_vec();
    let mut warnings = Vec::new();
    if normalized_legacy_bundle {
        warnings.push("xlsb legacy bundle-sheet layout normalized in memory".to_owned());
    }
    let supplement = match xlsb_styles::XlsbStyleSupplement::read(path) {
        Ok(supplement) => supplement,
        Err(message) => {
            warnings.push(format!(
                "xlsb number-format metadata could not be read; fmt values may be null: {message}"
            ));
            xlsb_styles::XlsbStyleSupplement::default()
        }
    };
    let mut emitted = 0_usize;
    let mut sheets = Vec::with_capacity(metadata.len());

    for (index, metadata) in metadata.iter().enumerate() {
        ensure_time_remaining(started, timeout)?;
        if metadata.typ != SheetType::WorkSheet {
            sheets.push(empty_sheet(&metadata.name, index));
            continue;
        }

        let mut records = BTreeMap::<(u32, u32), Candidate>::new();
        {
            let mut cells = workbook
                .worksheet_cells_reader(&metadata.name)
                .map_err(|error| container_error(WorkbookKind::Xlsb, error))?;
            let mut seen = 0_usize;
            while let Some(cell) = cells
                .next_cell()
                .map_err(|error| container_error(WorkbookKind::Xlsb, error))?
            {
                seen += 1;
                if seen.is_multiple_of(4096) {
                    ensure_time_remaining(started, timeout)?;
                }
                let position = cell.get_position();
                let candidate = records.entry(position).or_default();
                let value = Data::from(cell.get_value().clone());
                if !matches!(&value, Data::String(text) if text.is_empty()) {
                    candidate.value = value;
                }
                candidate.format = supplement.cell(&metadata.name, position).map(str::to_owned);
            }
        }
        {
            let mut formulas = workbook
                .worksheet_cells_reader(&metadata.name)
                .map_err(|error| container_error(WorkbookKind::Xlsb, error))?;
            let mut seen = 0_usize;
            loop {
                match formulas.next_formula() {
                    Ok(Some(cell)) => {
                        seen += 1;
                        if seen.is_multiple_of(4096) {
                            ensure_time_remaining(started, timeout)?;
                        }
                        records.entry(cell.get_position()).or_default().formula =
                            normalize_formula(cell.get_value().clone());
                    }
                    Ok(None) => break,
                    Err(error) => {
                        warnings.push(format!(
                            "xlsb formulas for sheet {:?} are partial: {error}",
                            metadata.name
                        ));
                        break;
                    }
                }
            }
        }
        if let Some(metadata) = supplement.cells(&metadata.name) {
            for (&position, metadata) in metadata {
                let candidate = records.entry(position).or_default();
                candidate.format = metadata.format.clone();
                if let Some(error) = metadata.cached_error.and_then(xlsb_error) {
                    candidate.value = Data::Error(error);
                } else if metadata.cached_empty_string {
                    candidate.value = Data::String(String::new());
                }
            }
        }

        let candidates = normalize_candidates(records, epoch_1904);
        let (rows, cols) = extent_from_cells(&candidates);
        sheets.push(finish_sheet(
            SheetDraft {
                name: &metadata.name,
                index,
                rows,
                cols,
                merges: Vec::new(),
                candidates,
            },
            &mut emitted,
            max_cells,
        ));
    }

    warnings.push("xlsb merged regions are best-effort".to_owned());
    Ok((sheets, warnings))
}

fn read_xls(
    path: &Path,
    max_cells: usize,
    started: Instant,
    timeout: Duration,
) -> Result<(Vec<Sheet>, Vec<String>), ReadFailure> {
    let mut workbook: Xls<BufReader<File>> =
        open_workbook(path).map_err(|error| container_error(WorkbookKind::Xls, error))?;
    let epoch_1904 = workbook.has_1904_epoch();
    let metadata = workbook.sheets_metadata().to_vec();
    let mut warnings = Vec::new();
    let supplement = match xls_styles::XlsStyleSupplement::read(path) {
        Ok(supplement) => supplement,
        Err(message) => {
            warnings.push(format!(
                "legacy xls number-format metadata could not be read; fmt values may be null: {message}"
            ));
            xls_styles::XlsStyleSupplement::default()
        }
    };
    let mut emitted = 0_usize;
    let mut sheets = Vec::with_capacity(metadata.len());

    for (index, metadata) in metadata.iter().enumerate() {
        ensure_time_remaining(started, timeout)?;
        if metadata.typ != SheetType::WorkSheet {
            sheets.push(empty_sheet(&metadata.name, index));
            continue;
        }
        let values = workbook
            .worksheet_range(&metadata.name)
            .map_err(|error| container_error(WorkbookKind::Xls, error))?;
        let formulas = workbook
            .worksheet_formula(&metadata.name)
            .map_err(|error| container_error(WorkbookKind::Xls, error))?;
        let (mut records, rows, cols) = range_records(&values, &formulas, true);
        for &position in supplement.empty_formula_cells(index) {
            let candidate = records.entry(position).or_default();
            if matches!(candidate.value, Data::Empty) {
                candidate.value = Data::String(String::new());
            }
        }
        for (position, candidate) in &mut records {
            candidate.format = supplement.cell(index, *position).map(str::to_owned);
        }
        let merges = workbook
            .merge_cells_by_sheet_name(&metadata.name)
            .map_err(|error| container_error(WorkbookKind::Xls, error))?;
        sheets.push(finish_sheet(
            SheetDraft {
                name: &metadata.name,
                index,
                rows,
                cols,
                merges,
                candidates: normalize_candidates(records, epoch_1904),
            },
            &mut emitted,
            max_cells,
        ));
    }

    Ok((sheets, warnings))
}

fn read_ods(
    path: &Path,
    max_cells: usize,
    started: Instant,
    timeout: Duration,
) -> Result<(Vec<Sheet>, Vec<String>), ReadFailure> {
    let mut workbook: Ods<BufReader<File>> =
        open_workbook(path).map_err(|error| container_error(WorkbookKind::Ods, error))?;
    let metadata = workbook.sheets_metadata().to_vec();
    let mut emitted = 0_usize;
    let mut sheets = Vec::with_capacity(metadata.len());

    for (index, metadata) in metadata.iter().enumerate() {
        ensure_time_remaining(started, timeout)?;
        let values = workbook
            .worksheet_range(&metadata.name)
            .map_err(|error| container_error(WorkbookKind::Ods, error))?;
        let formulas = workbook
            .worksheet_formula(&metadata.name)
            .map_err(|error| container_error(WorkbookKind::Ods, error))?;
        // ODS has no style supplement to restore empty cached strings, so
        // they must survive the values pass here.
        let (records, rows, cols) = range_records(&values, &formulas, false);
        sheets.push(finish_sheet(
            SheetDraft {
                name: &metadata.name,
                index,
                rows,
                cols,
                merges: Vec::new(),
                candidates: normalize_candidates(records, false),
            },
            &mut emitted,
            max_cells,
        ));
    }

    Ok((
        sheets,
        vec!["ods number-format codes and merged regions are best-effort".to_owned()],
    ))
}

#[derive(Default)]
struct Candidate {
    value: Data,
    formula: Option<String>,
    format: Option<String>,
}

fn range_records(
    values: &Range<Data>,
    formulas: &Range<String>,
    skip_empty_strings: bool,
) -> (BTreeMap<(u32, u32), Candidate>, u32, u32) {
    let mut records = BTreeMap::<(u32, u32), Candidate>::new();
    let value_start = values.start().unwrap_or((0, 0));
    for (row, col, value) in values.used_cells() {
        let position = (
            value_start.0.saturating_add(row as u32),
            value_start.1.saturating_add(col as u32),
        );
        if !(skip_empty_strings && matches!(value, Data::String(text) if text.is_empty())) {
            records.entry(position).or_default().value = value.clone();
        }
    }
    let formula_start = formulas.start().unwrap_or((0, 0));
    for (row, col, formula) in formulas.used_cells() {
        let position = (
            formula_start.0.saturating_add(row as u32),
            formula_start.1.saturating_add(col as u32),
        );
        records.entry(position).or_default().formula = normalize_formula(formula.clone());
    }

    let end = match (values.end(), formulas.end()) {
        (Some(left), Some(right)) => Some((left.0.max(right.0), left.1.max(right.1))),
        (Some(end), None) | (None, Some(end)) => Some(end),
        (None, None) => None,
    };
    let (rows, cols) = end.map_or((0, 0), |(row, col)| {
        (row.saturating_add(1), col.saturating_add(1))
    });
    (records, rows, cols)
}

fn normalize_candidates(records: BTreeMap<(u32, u32), Candidate>, epoch_1904: bool) -> Vec<Cell> {
    records
        .into_iter()
        .filter_map(|(position, candidate)| {
            normalize_cell(
                position,
                &candidate.value,
                candidate.formula,
                candidate.format.as_deref(),
                None,
                epoch_1904,
            )
        })
        .collect()
}

fn normalize_cell(
    position: (u32, u32),
    data: &Data,
    formula: Option<String>,
    format: Option<&str>,
    declared_type: Option<CellType>,
    epoch_1904: bool,
) -> Option<Cell> {
    let is_empty =
        matches!(data, Data::Empty) || matches!(data, Data::Float(value) if !value.is_finite());
    if formula.is_none() && is_empty {
        return None;
    }

    let code = format.unwrap_or("General");
    let (cell_type, value, display) = match data {
        Data::Int(value) => {
            let raw = *value as f64;
            let (cell_type, value) = normalize_number_value(raw, code, epoch_1904);
            (
                cell_type,
                Some(value),
                render(code, FmtValue::Number(raw), epoch_1904),
            )
        }
        Data::Float(value) if value.is_finite() => {
            let raw = *value;
            let (cell_type, value) = normalize_number_value(raw, code, epoch_1904);
            (
                cell_type,
                Some(value),
                render(code, FmtValue::Number(raw), epoch_1904),
            )
        }
        Data::Float(_) => (CellType::N, None, None),
        Data::String(value) => (
            CellType::S,
            Some(CellValue::Text(value.clone())),
            render(code, FmtValue::Text(value), epoch_1904),
        ),
        Data::Bool(value) => (
            CellType::B,
            Some(CellValue::Bool(*value)),
            render(code, FmtValue::Bool(*value), epoch_1904),
        ),
        Data::Error(error) => {
            let value = error_text(error);
            (
                CellType::E,
                Some(CellValue::Text(value.to_owned())),
                render(code, FmtValue::Error(value), epoch_1904),
            )
        }
        Data::DateTime(value) if value.is_datetime() => {
            // calamine has already typed this cell as a datetime; do not let
            // a missing/degraded format supplement flip it back to numeric.
            // Only negative serials downgrade: Excel's date systems cannot
            // render them as dates, so the number is kept losslessly.
            let serial = value.as_f64();
            let (cell_type, value) = if serial >= 0.0 {
                (CellType::D, CellValue::Text(excel_datetime_iso(*value)))
            } else {
                (CellType::N, CellValue::Number(serial))
            };
            (
                cell_type,
                Some(value),
                render(code, FmtValue::Number(serial), epoch_1904),
            )
        }
        Data::DateTime(value) => {
            let serial = value.as_f64();
            (
                CellType::N,
                Some(CellValue::Number(serial)),
                render(code, FmtValue::Number(serial), epoch_1904),
            )
        }
        Data::DateTimeIso(value) => (
            CellType::D,
            Some(CellValue::Text(normalize_iso_datetime(value))),
            render(code, FmtValue::Text(value), epoch_1904),
        ),
        Data::DurationIso(value) => (
            CellType::S,
            Some(CellValue::Text(value.clone())),
            render(code, FmtValue::Text(value), epoch_1904),
        ),
        Data::Empty => (declared_type.unwrap_or(CellType::S), None, None),
    };

    Some(Cell {
        s: None,
        r: position.0,
        c: position.1,
        t: cell_type,
        v: value,
        d: display,
        f: formula,
        fmt: explicit_format(format),
    })
}

fn normalize_number_value(raw: f64, code: &str, epoch_1904: bool) -> (CellType, CellValue) {
    // Excel's date systems do not render negative serials as dates. Keep the
    // stored number losslessly instead of clamping it to the epoch date.
    if raw >= 0.0 && number_format_kind(code) == NumberFormatKind::DateTime {
        let value = ExcelDateTime::new(raw, ExcelDateTimeType::DateTime, epoch_1904);
        (CellType::D, CellValue::Text(excel_datetime_iso(value)))
    } else {
        (CellType::N, CellValue::Number(raw))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NumberFormatKind {
    Other,
    DateTime,
    Duration,
}

// Mirrors Excel's format-token rules conservatively. This supplements
// calamine's type inference when the container parser did not connect a
// custom XF to its format code; it never participates in display rendering.
fn number_format_kind(code: &str) -> NumberFormatKind {
    let mut escaped = false;
    let mut quoted = false;
    let mut brackets = 0_u8;
    let mut previous = ' ';
    let mut duration_token = false;
    let mut am_pm = false;

    for character in code.chars() {
        match (character, escaped, quoted, am_pm, brackets) {
            (_, true, ..) => escaped = false,
            ('_' | '\\' | '*', ..) => escaped = true,
            ('"', _, true, _, _) => quoted = false,
            (_, _, true, _, _) => {}
            ('"', _, _, _, _) => quoted = true,
            (';', ..) => return NumberFormatKind::Other,
            ('[', ..) => brackets = brackets.saturating_add(1),
            (']', .., 1) if duration_token => return NumberFormatKind::Duration,
            (']', ..) => brackets = brackets.saturating_sub(1),
            ('a' | 'A', _, _, false, 0) => am_pm = true,
            ('p' | 'm' | '/' | 'P' | 'M', _, _, true, 0) => {
                return NumberFormatKind::DateTime;
            }
            ('d' | 'm' | 'h' | 'y' | 's' | 'D' | 'M' | 'H' | 'Y' | 'S', _, _, false, 0) => {
                return NumberFormatKind::DateTime;
            }
            _ => {
                if duration_token && character.eq_ignore_ascii_case(&previous) {
                    // Repeated h/m/s inside brackets remains a duration token.
                } else {
                    duration_token =
                        previous == '[' && matches!(character, 'm' | 'h' | 's' | 'M' | 'H' | 'S');
                }
            }
        }
        previous = character;
    }
    NumberFormatKind::Other
}

fn normalize_formula(formula: String) -> Option<String> {
    let formula = formula
        .strip_prefix("of:=")
        .or_else(|| formula.strip_prefix("oooc:="))
        .or_else(|| formula.strip_prefix('='))
        .unwrap_or(&formula);
    (!formula.is_empty()).then(|| formula.to_owned())
}

fn normalize_iso_datetime(value: &str) -> String {
    for suffix in ["T00:00:00.000", "T00:00:00"] {
        if let Some(date) = value.strip_suffix(suffix) {
            return date.to_owned();
        }
    }
    value.strip_suffix('Z').unwrap_or(value).to_owned()
}

fn excel_datetime_iso(value: calamine::ExcelDateTime) -> String {
    let (year, month, day, hour, minute, second, millisecond) = value.to_ymd_hms_milli();
    if hour == 0 && minute == 0 && second == 0 && millisecond == 0 {
        format!("{year:04}-{month:02}-{day:02}")
    } else if millisecond == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
    } else {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}")
    }
}

fn error_text(error: &calamine::CellErrorType) -> &'static str {
    match error {
        calamine::CellErrorType::Div0 => "#DIV/0!",
        calamine::CellErrorType::NA => "#N/A",
        calamine::CellErrorType::Name => "#NAME?",
        calamine::CellErrorType::Null => "#NULL!",
        calamine::CellErrorType::Num => "#NUM!",
        calamine::CellErrorType::Ref => "#REF!",
        calamine::CellErrorType::Value => "#VALUE!",
        calamine::CellErrorType::GettingData => "#GETTING_DATA",
    }
}

fn xlsb_error(error: u8) -> Option<calamine::CellErrorType> {
    match error {
        0x00 => Some(calamine::CellErrorType::Null),
        0x07 => Some(calamine::CellErrorType::Div0),
        0x0F => Some(calamine::CellErrorType::Value),
        0x17 => Some(calamine::CellErrorType::Ref),
        0x1D => Some(calamine::CellErrorType::Name),
        0x24 => Some(calamine::CellErrorType::Num),
        0x2A => Some(calamine::CellErrorType::NA),
        0x2B => Some(calamine::CellErrorType::GettingData),
        _ => None,
    }
}

fn explicit_format(format: Option<&str>) -> Option<String> {
    format
        .filter(|format| !format.eq_ignore_ascii_case("general"))
        .map(str::to_owned)
}

fn intern_style(styles: &mut Vec<CellStyle>, style: &CellStyle) -> Option<u32> {
    if style == &CellStyle::default() {
        return None;
    }
    let index = styles
        .iter()
        .position(|candidate| candidate == style)
        .unwrap_or_else(|| {
            styles.push(style.clone());
            styles.len() - 1
        });
    u32::try_from(index).ok()
}

fn compact_styles(sheets: &mut [Sheet], styles: &[CellStyle]) -> Vec<CellStyle> {
    let mut compact = Vec::new();
    for cell in sheets.iter_mut().flat_map(|sheet| &mut sheet.cells) {
        cell.s = cell
            .s
            .and_then(|index| styles.get(usize::try_from(index).ok()?))
            .and_then(|style| intern_style(&mut compact, style));
    }
    compact
}

struct SheetDraft<'a> {
    name: &'a str,
    index: usize,
    rows: u32,
    cols: u32,
    merges: Vec<Dimensions>,
    candidates: Vec<Cell>,
}

fn finish_sheet(draft: SheetDraft<'_>, emitted: &mut usize, max_cells: usize) -> Sheet {
    let SheetDraft {
        name,
        index,
        rows,
        cols,
        merges,
        mut candidates,
    } = draft;
    candidates.sort_by_key(|cell| (cell.r, cell.c));
    let remaining = max_cells.saturating_sub(*emitted);
    let truncated = candidates.len() > remaining;
    if truncated {
        candidates.truncate(remaining);
    }
    *emitted = emitted.saturating_add(candidates.len());
    let mut merges = merges.into_iter().map(a1_range).collect::<Vec<_>>();
    merges.sort();
    Sheet {
        col_infos: Vec::new(),
        name: name.to_owned(),
        index: index as u32,
        rows,
        cols,
        truncated,
        merges,
        cells: candidates,
    }
}

fn empty_sheet(name: &str, index: usize) -> Sheet {
    Sheet {
        col_infos: Vec::new(),
        name: name.to_owned(),
        index: index as u32,
        rows: 0,
        cols: 0,
        truncated: false,
        merges: Vec::new(),
        cells: Vec::new(),
    }
}

fn extent_from_dimensions(dimensions: Dimensions, cells: &[Cell]) -> (u32, u32) {
    let dimension_extent = (
        dimensions.end.0.saturating_add(1),
        dimensions.end.1.saturating_add(1),
    );
    let cell_extent = extent_from_cells(cells);
    (
        dimension_extent.0.max(cell_extent.0),
        dimension_extent.1.max(cell_extent.1),
    )
}

fn extent_from_cells(cells: &[Cell]) -> (u32, u32) {
    cells.iter().fold((0, 0), |(rows, cols), cell| {
        (
            rows.max(cell.r.saturating_add(1)),
            cols.max(cell.c.saturating_add(1)),
        )
    })
}

fn a1_range(dimensions: Dimensions) -> String {
    let start = a1_cell(dimensions.start);
    let end = a1_cell(dimensions.end);
    if start == end {
        start
    } else {
        format!("{start}:{end}")
    }
}

fn a1_cell((row, col): (u32, u32)) -> String {
    let mut col = u64::from(col) + 1;
    let mut letters = Vec::new();
    while col > 0 {
        let remainder = ((col - 1) % 26) as u8;
        letters.push((b'A' + remainder) as char);
        col = (col - 1) / 26;
    }
    letters.reverse();
    format!(
        "{}{row}",
        letters.into_iter().collect::<String>(),
        row = row + 1
    )
}

fn ensure_time_remaining(started: Instant, timeout: Duration) -> Result<(), ReadFailure> {
    if started.elapsed() >= timeout {
        Err(ReadFailure::new(
            ErrorCode::Timeout,
            "workbook parse timed out",
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug)]
struct ReadFailure {
    code: ErrorCode,
    message: String,
}

impl ReadFailure {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

fn failure_document(file: String, failure: ReadFailure) -> Document {
    Document::failure(
        env!("CARGO_PKG_VERSION"),
        file,
        DumpError {
            code: failure.code.as_str().to_owned(),
            msg: failure.message,
        },
        Vec::new(),
    )
}

fn container_error(kind: WorkbookKind, error: impl std::fmt::Display) -> ReadFailure {
    let message = error.to_string();
    let lowercase = message.to_ascii_lowercase();
    let code = if lowercase.contains("password") || lowercase.contains("unsupported") {
        ErrorCode::Unsupported
    } else if matches!(
        kind,
        WorkbookKind::Xlsx | WorkbookKind::Xlsb | WorkbookKind::Ods
    ) || (matches!(kind, WorkbookKind::Xls) && is_structural_xls_error(&lowercase))
    {
        ErrorCode::BadZip
    } else {
        ErrorCode::Internal
    };
    ReadFailure::new(code, message)
}

fn is_structural_xls_error(message: &str) -> bool {
    message.contains("cfb error:")
        || message.contains("invalid short string length")
        || message.contains("invalid long string length")
        || message.contains("invalid compound")
        || message.contains("sector ") && message.contains("past end of file")
}

#[derive(Clone, Copy, Debug)]
struct CellMetadata {
    style_index: Option<usize>,
    declared_type: Option<CellType>,
}

#[derive(Clone, Copy)]
struct ResolvedCellMetadata<'a> {
    format: Option<&'a str>,
    style: Option<&'a CellStyle>,
    declared_type: Option<CellType>,
}

#[derive(Clone, Debug, Default)]
struct ResolvedXf {
    format: Option<String>,
    style: Option<CellStyle>,
}

enum StyleAttribute<T> {
    Missing,
    Value(T),
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ColumnDeclaration {
    min: u32,
    max: u32,
    width: f64,
}

#[derive(Default)]
struct OoxmlSheetSupplement {
    cells: HashMap<(u32, u32), CellMetadata>,
    columns: Vec<ColumnDeclaration>,
}

#[derive(Default)]
struct OoxmlSupplement {
    sheets: HashMap<String, OoxmlSheetSupplement>,
    xfs: Vec<ResolvedXf>,
    warnings: Vec<String>,
}

impl OoxmlSupplement {
    fn read(path: &Path) -> Result<Self, String> {
        let input = File::open(path).map_err(|error| format!("could not open package: {error}"))?;
        let mut archive =
            ZipArchive::new(BufReader::new(input)).map_err(|error| format!("bad zip: {error}"))?;
        let lookup = zip_lookup(&mut archive)?;

        let workbook_path = read_part_optional(&mut archive, &lookup, "_rels/.rels")?
            .and_then(|xml| {
                parse_relationships(&xml).ok().and_then(|relationships| {
                    relationships
                        .into_iter()
                        .find(|relationship| relationship.kind.ends_with("/officeDocument"))
                        .map(|relationship| resolve_part("", &relationship.target))
                })
            })
            .unwrap_or_else(|| "xl/workbook.xml".to_owned());
        let workbook_xml = read_part(&mut archive, &lookup, &workbook_path)?;
        let workbook_sheets = parse_workbook_sheets(&workbook_xml)?;
        let workbook_relationships_path = relationships_path(&workbook_path);
        let relationships_xml = read_part(&mut archive, &lookup, &workbook_relationships_path)?;
        let relationships = parse_relationships(&relationships_xml)?;

        let styles_path = relationships
            .iter()
            .find(|relationship| relationship.kind.ends_with("/styles"))
            .map(|relationship| resolve_part(&workbook_path, &relationship.target))
            .unwrap_or_else(|| resolve_part(&workbook_path, "styles.xml"));
        let mut warnings = Vec::new();
        let xfs = match read_part_optional(&mut archive, &lookup, &styles_path)? {
            Some(styles) => match parse_styles(&styles) {
                Ok(xfs) => xfs,
                Err(message) => {
                    warnings.push(format!(
                        "xlsx styles could not be read; fmt and style values may be null: {message}"
                    ));
                    Vec::new()
                }
            },
            None => Vec::new(),
        };

        let by_id = relationships
            .into_iter()
            .map(|relationship| (relationship.id, relationship.target))
            .collect::<HashMap<_, _>>();
        let mut sheets = HashMap::new();
        for workbook_sheet in workbook_sheets {
            let Some(target) = by_id.get(&workbook_sheet.relationship_id) else {
                continue;
            };
            let sheet_path = resolve_part(&workbook_path, target);
            let Some(xml) = read_part_optional(&mut archive, &lookup, &sheet_path)? else {
                continue;
            };
            let parsed = match parse_sheet_metadata(&xml) {
                Ok(parsed) => parsed,
                Err(message) => {
                    warnings.push(format!(
                        "xlsx cell metadata for sheet {:?} could not be read; fmt and style values may be null: {message}",
                        workbook_sheet.name
                    ));
                    warnings.push(format!(
                        "xlsx column widths for sheet {:?} could not be read: {message}",
                        workbook_sheet.name
                    ));
                    ParsedSheetMetadata {
                        cells: HashMap::new(),
                        columns: Ok(Vec::new()),
                    }
                }
            };
            let columns = match parsed.columns {
                Ok(columns) => columns,
                Err(message) => {
                    warnings.push(format!(
                        "xlsx column widths for sheet {:?} could not be read: {message}",
                        workbook_sheet.name
                    ));
                    Vec::new()
                }
            };
            sheets.insert(
                workbook_sheet.name,
                OoxmlSheetSupplement {
                    cells: parsed.cells,
                    columns,
                },
            );
        }
        Ok(Self {
            sheets,
            xfs,
            warnings,
        })
    }

    fn cell(&self, sheet: &str, position: (u32, u32)) -> Option<ResolvedCellMetadata<'_>> {
        let metadata = self.sheets.get(sheet)?.cells.get(&position)?;
        // OOXML defines a missing `s` attribute as cell XF zero. This matters
        // when the workbook's base font is explicit (as it normally is), even
        // though XF zero's number format is usually General.
        let xf = self.xfs.get(metadata.style_index.unwrap_or(0));
        Some(ResolvedCellMetadata {
            format: xf.and_then(|xf| xf.format.as_deref()),
            style: xf.and_then(|xf| xf.style.as_ref()),
            declared_type: metadata.declared_type,
        })
    }

    fn col_infos(&self, sheet: &str, used_cols: u32) -> Vec<ColInfo> {
        const EXCEL_MAX_COLUMNS: u32 = 16_384;
        const WIDTH_HEADROOM_COLUMNS: u32 = 256;

        let Some(sheet) = self.sheets.get(sheet) else {
            return Vec::new();
        };
        let expansion_limit = used_cols
            .min(EXCEL_MAX_COLUMNS)
            .saturating_add(WIDTH_HEADROOM_COLUMNS)
            .min(EXCEL_MAX_COLUMNS);
        let limit = usize::try_from(expansion_limit).unwrap_or(usize::MAX);
        let mut widths = vec![None; limit];
        // A successor set skips columns that already received their
        // last-declaration-wins value. Iterating declarations in reverse makes
        // each output column writable once, bounding expansion by O(N + cap)
        // even when thousands of declarations cover the same whole sheet.
        let mut next_uncovered = (0..=limit).collect::<Vec<_>>();
        let mut remaining = limit;
        for declaration in sheet.columns.iter().rev() {
            if remaining == 0 {
                break;
            }
            let start = usize::try_from(declaration.min.saturating_sub(1)).unwrap_or(usize::MAX);
            let end = usize::try_from(declaration.max.min(expansion_limit).saturating_sub(1))
                .unwrap_or(usize::MAX);
            if start >= limit || start > end {
                continue;
            }
            let mut column = next_available_column(&mut next_uncovered, start);
            while column <= end {
                widths[column] = Some(declaration.width);
                remaining -= 1;
                let successor = next_available_column(&mut next_uncovered, column + 1);
                next_uncovered[column] = successor;
                column = successor;
            }
        }
        widths
            .into_iter()
            .enumerate()
            .filter_map(|(column, width)| {
                Some(ColInfo {
                    c: u32::try_from(column).ok()?,
                    width: width?,
                })
            })
            .collect()
    }
}

fn next_available_column(successors: &mut [usize], index: usize) -> usize {
    let mut root = index;
    while successors[root] != root {
        root = successors[root];
    }
    let mut current = index;
    while successors[current] != current {
        let next = successors[current];
        successors[current] = root;
        current = next;
    }
    root
}

struct WorkbookSheet {
    name: String,
    relationship_id: String,
}

struct Relationship {
    id: String,
    target: String,
    kind: String,
}

fn zip_lookup<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<HashMap<String, String>, String> {
    let mut lookup = HashMap::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("could not inspect zip entry: {error}"))?;
        lookup.insert(zip_part_key(entry.name()), entry.name().to_owned());
    }
    Ok(lookup)
}

fn zip_part_key(path: &str) -> String {
    path.trim_start_matches(['/', '\\'])
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn read_part<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    lookup: &HashMap<String, String>,
    path: &str,
) -> Result<Vec<u8>, String> {
    read_part_optional(archive, lookup, path)?.ok_or_else(|| format!("missing OOXML part {path}"))
}

fn read_part_optional<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    lookup: &HashMap<String, String>,
    path: &str,
) -> Result<Option<Vec<u8>>, String> {
    let normalized = zip_part_key(path);
    let Some(actual_path) = lookup.get(&normalized) else {
        return Ok(None);
    };
    let mut entry = archive
        .by_name(actual_path)
        .map_err(|error| format!("could not open {path}: {error}"))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read {path}: {error}"))?;
    Ok(Some(bytes))
}

fn normalize_nonstandard_workbook_package(path: &Path) -> Result<Option<Cursor<Vec<u8>>>, String> {
    let input = File::open(path).map_err(|error| format!("could not open package: {error}"))?;
    let mut archive =
        ZipArchive::new(BufReader::new(input)).map_err(|error| format!("bad zip: {error}"))?;
    let lookup = zip_lookup(&mut archive)?;
    let Some(root_relationships) = read_part_optional(&mut archive, &lookup, "_rels/.rels")? else {
        return Ok(None);
    };
    let Some(workbook_path) = parse_relationships(&root_relationships)?
        .into_iter()
        .find(|relationship| relationship.kind.ends_with("/officeDocument"))
        .map(|relationship| resolve_part("", &relationship.target))
    else {
        return Ok(None);
    };
    let directory = workbook_path
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let standard_workbook_path = if directory.is_empty() {
        "workbook.xml".to_owned()
    } else {
        format!("{directory}/workbook.xml")
    };
    if zip_part_key(&workbook_path) == zip_part_key(&standard_workbook_path) {
        return Ok(None);
    }
    let workbook = read_part(&mut archive, &lookup, &workbook_path)?;
    let workbook_relationships_path = relationships_path(&workbook_path);
    let workbook_relationships = read_part(&mut archive, &lookup, &workbook_relationships_path)?;
    let standard_relationships_path = relationships_path(&standard_workbook_path);

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("could not inspect zip entry: {error}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| format!("could not read {name}: {error}"))?;
        writer
            .start_file(&name, SimpleFileOptions::default())
            .map_err(|error| format!("could not normalize {name}: {error}"))?;
        writer
            .write_all(&bytes)
            .map_err(|error| format!("could not normalize {name}: {error}"))?;
    }
    for (name, bytes) in [
        (standard_workbook_path, workbook),
        (standard_relationships_path, workbook_relationships),
    ] {
        writer
            .start_file(&name, SimpleFileOptions::default())
            .map_err(|error| format!("could not add {name}: {error}"))?;
        writer
            .write_all(&bytes)
            .map_err(|error| format!("could not add {name}: {error}"))?;
    }
    let mut reader = writer
        .finish()
        .map_err(|error| format!("could not finish normalized package: {error}"))?;
    reader.set_position(0);
    Ok(Some(reader))
}

fn parse_workbook_sheets(xml: &[u8]) -> Result<Vec<WorkbookSheet>, String> {
    let mut reader = XmlReader::from_reader(xml);
    let mut buffer = Vec::new();
    let mut sheets = Vec::new();
    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"sheet" =>
            {
                let name = xml_attribute(&reader, &element, b"name")?
                    .ok_or_else(|| "workbook sheet has no name".to_owned())?;
                let relationship_id = xml_attribute(&reader, &element, b"r:id")?
                    .or_else(|| {
                        xml_attribute_by_local_name(&reader, &element, b"id")
                            .ok()
                            .flatten()
                    })
                    .ok_or_else(|| format!("workbook sheet {name:?} has no relationship id"))?;
                sheets.push(WorkbookSheet {
                    name,
                    relationship_id,
                });
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("invalid workbook XML: {error}")),
        }
    }
    Ok(sheets)
}

fn parse_relationships(xml: &[u8]) -> Result<Vec<Relationship>, String> {
    let mut reader = XmlReader::from_reader(xml);
    let mut buffer = Vec::new();
    let mut relationships = Vec::new();
    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"Relationship" =>
            {
                if let (Some(id), Some(target)) = (
                    xml_attribute(&reader, &element, b"Id")?,
                    xml_attribute(&reader, &element, b"Target")?,
                ) {
                    relationships.push(Relationship {
                        id,
                        target,
                        kind: xml_attribute(&reader, &element, b"Type")?.unwrap_or_default(),
                    });
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("invalid relationships XML: {error}")),
        }
    }
    Ok(relationships)
}

fn parse_styles(xml: &[u8]) -> Result<Vec<ResolvedXf>, String> {
    let mut reader = XmlReader::from_reader(xml);
    let mut buffer = Vec::new();
    let mut custom = HashMap::<u32, String>::new();
    let mut fonts = Vec::new();
    let mut fills = Vec::new();
    let mut xfs = Vec::new();
    let mut in_fonts = false;
    let mut in_fills = false;
    let mut in_num_formats = false;
    let mut in_cell_xfs = false;
    let mut saw_style_sheet = false;
    let mut closed_style_sheet = false;

    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) if element.local_name().as_ref() == b"styleSheet" => {
                saw_style_sheet = true;
            }
            Ok(Event::Empty(element)) if element.local_name().as_ref() == b"styleSheet" => {
                saw_style_sheet = true;
                closed_style_sheet = true;
            }
            Ok(Event::End(element)) if element.local_name().as_ref() == b"styleSheet" => {
                closed_style_sheet = true;
            }
            Ok(Event::Start(element)) if element.local_name().as_ref() == b"fonts" => {
                in_fonts = true;
            }
            Ok(Event::End(element)) if element.local_name().as_ref() == b"fonts" => {
                in_fonts = false;
            }
            Ok(Event::Start(element)) if element.local_name().as_ref() == b"fills" => {
                in_fills = true;
            }
            Ok(Event::End(element)) if element.local_name().as_ref() == b"fills" => {
                in_fills = false;
            }
            Ok(Event::Start(element)) if element.local_name().as_ref() == b"numFmts" => {
                in_num_formats = true;
            }
            Ok(Event::End(element)) if element.local_name().as_ref() == b"numFmts" => {
                in_num_formats = false;
            }
            Ok(Event::Start(element)) if element.local_name().as_ref() == b"cellXfs" => {
                in_cell_xfs = true;
            }
            Ok(Event::End(element)) if element.local_name().as_ref() == b"cellXfs" => {
                in_cell_xfs = false;
            }
            Ok(Event::Start(element)) if in_fonts && element.local_name().as_ref() == b"font" => {
                fonts.push(parse_font(&mut reader)?);
            }
            Ok(Event::Empty(element)) if in_fonts && element.local_name().as_ref() == b"font" => {
                fonts.push(CellStyle::default());
            }
            Ok(Event::Start(element)) if in_fills && element.local_name().as_ref() == b"fill" => {
                fills.push(parse_fill(&mut reader)?);
            }
            Ok(Event::Empty(element)) if in_fills && element.local_name().as_ref() == b"fill" => {
                fills.push(None);
            }
            Ok(Event::Start(element) | Event::Empty(element))
                if in_num_formats && element.local_name().as_ref() == b"numFmt" =>
            {
                if let (Some(id), Some(code)) = (
                    style_attribute_value(style_u32_attribute(&reader, &element, b"numFmtId")?),
                    xml_attribute(&reader, &element, b"formatCode")?,
                ) {
                    custom.insert(id, code);
                }
            }
            Ok(Event::Start(element) | Event::Empty(element))
                if in_cell_xfs && element.local_name().as_ref() == b"xf" =>
            {
                let Some(id) =
                    style_id_or_default(style_u32_attribute(&reader, &element, b"numFmtId")?)
                else {
                    xfs.push(ResolvedXf::default());
                    continue;
                };
                let Some(font_id) =
                    style_id_or_default(style_u32_attribute(&reader, &element, b"fontId")?)
                else {
                    xfs.push(ResolvedXf::default());
                    continue;
                };
                let Some(fill_id) =
                    style_id_or_default(style_u32_attribute(&reader, &element, b"fillId")?)
                else {
                    xfs.push(ResolvedXf::default());
                    continue;
                };
                let format = custom
                    .get(&id)
                    .cloned()
                    .or_else(|| builtin_format(id).map(str::to_owned));
                let mut style = fonts
                    .get(usize::try_from(font_id).unwrap_or(usize::MAX))
                    .cloned()
                    .unwrap_or_default();
                style.fill_color = fills
                    .get(usize::try_from(fill_id).unwrap_or(usize::MAX))
                    .cloned()
                    .flatten();
                xfs.push(ResolvedXf {
                    format: explicit_format(format.as_deref()),
                    style: (style != CellStyle::default()).then_some(style),
                });
            }
            Ok(Event::Eof) => {
                if !saw_style_sheet {
                    return Err("invalid styles XML: missing styleSheet element".to_owned());
                }
                if !closed_style_sheet {
                    return Err("invalid styles XML: unclosed styleSheet element".to_owned());
                }
                break;
            }
            Ok(_) => {}
            Err(error) => return Err(format!("invalid styles XML: {error}")),
        }
    }
    Ok(xfs)
}

fn parse_font(reader: &mut XmlReader<&[u8]>) -> Result<CellStyle, String> {
    let mut buffer = Vec::new();
    let mut style = CellStyle::default();
    let mut invalid = false;
    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element) | Event::Empty(element)) => {
                match element.local_name().as_ref() {
                    b"b" => match style_on_off_attribute(reader, &element)? {
                        StyleAttribute::Value(value) => style.bold = value,
                        StyleAttribute::Missing | StyleAttribute::Invalid => invalid = true,
                    },
                    b"i" => match style_on_off_attribute(reader, &element)? {
                        StyleAttribute::Value(value) => style.italic = value,
                        StyleAttribute::Missing | StyleAttribute::Invalid => invalid = true,
                    },
                    b"u" => match style_underline_attribute(reader, &element)? {
                        StyleAttribute::Value(value) => style.underline = value,
                        StyleAttribute::Missing | StyleAttribute::Invalid => invalid = true,
                    },
                    b"strike" => match style_on_off_attribute(reader, &element)? {
                        StyleAttribute::Value(value) => style.strike = value,
                        StyleAttribute::Missing | StyleAttribute::Invalid => invalid = true,
                    },
                    b"sz" => match style_f64_attribute(reader, &element, b"val")? {
                        StyleAttribute::Value(size) if size > 0.0 => {
                            style.font_size = Some(size);
                        }
                        StyleAttribute::Missing
                        | StyleAttribute::Value(_)
                        | StyleAttribute::Invalid => invalid = true,
                    },
                    b"name" => match xml_attribute(reader, &element, b"val")? {
                        Some(name) if !name.is_empty() => style.font_name = Some(name),
                        Some(_) | None => invalid = true,
                    },
                    b"color" => match parse_ooxml_color(reader, &element)? {
                        StyleAttribute::Value(color) => style.font_color = color,
                        StyleAttribute::Missing => {}
                        StyleAttribute::Invalid => invalid = true,
                    },
                    _ => {}
                }
            }
            Ok(Event::End(element)) if element.local_name().as_ref() == b"font" => break,
            Ok(Event::Eof) => return Err("invalid styles XML: unclosed font element".to_owned()),
            Ok(_) => {}
            Err(error) => return Err(format!("invalid styles XML: {error}")),
        }
    }
    Ok(if invalid { CellStyle::default() } else { style })
}

fn parse_fill(reader: &mut XmlReader<&[u8]>) -> Result<Option<String>, String> {
    let mut buffer = Vec::new();
    let mut solid = false;
    let mut foreground = None;
    let mut invalid = false;
    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"patternFill" =>
            {
                solid = xml_attribute(reader, &element, b"patternType")?
                    .is_some_and(|pattern| pattern.eq_ignore_ascii_case("solid"));
            }
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"fgColor" =>
            {
                match parse_ooxml_color(reader, &element)? {
                    StyleAttribute::Value(color) => foreground = color,
                    StyleAttribute::Missing => {}
                    StyleAttribute::Invalid => invalid = true,
                }
            }
            Ok(Event::End(element)) if element.local_name().as_ref() == b"fill" => break,
            Ok(Event::Eof) => return Err("invalid styles XML: unclosed fill element".to_owned()),
            Ok(_) => {}
            Err(error) => return Err(format!("invalid styles XML: {error}")),
        }
    }
    Ok((solid && !invalid).then_some(foreground).flatten())
}

fn parse_ooxml_color(
    reader: &XmlReader<&[u8]>,
    element: &BytesStart<'_>,
) -> Result<StyleAttribute<Option<String>>, String> {
    if xml_attribute(reader, element, b"theme")?.is_some()
        || xml_attribute(reader, element, b"tint")?.is_some()
    {
        return Ok(StyleAttribute::Value(None));
    }
    if let Some(rgb) = xml_attribute(reader, element, b"rgb")? {
        let rgb = match rgb.len() {
            6 => rgb.as_str(),
            8 => &rgb[2..],
            _ => return Ok(StyleAttribute::Invalid),
        };
        if !rgb.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(StyleAttribute::Invalid);
        }
        return Ok(StyleAttribute::Value(Some(format!(
            "#{}",
            rgb.to_ascii_uppercase()
        ))));
    }
    let Some(indexed) = xml_attribute(reader, element, b"indexed")? else {
        return Ok(StyleAttribute::Missing);
    };
    let Ok(indexed) = indexed.parse::<usize>() else {
        return Ok(StyleAttribute::Invalid);
    };
    Ok(StyleAttribute::Value(
        LEGACY_INDEXED_COLORS
            .get(indexed)
            .map(|rgb| format!("#{rgb}")),
    ))
}

fn style_attribute_value<T>(attribute: StyleAttribute<T>) -> Option<T> {
    match attribute {
        StyleAttribute::Value(value) => Some(value),
        StyleAttribute::Missing | StyleAttribute::Invalid => None,
    }
}

fn style_id_or_default(attribute: StyleAttribute<u32>) -> Option<u32> {
    match attribute {
        StyleAttribute::Missing => Some(0),
        StyleAttribute::Value(value) => Some(value),
        StyleAttribute::Invalid => None,
    }
}

fn style_u32_attribute(
    reader: &XmlReader<&[u8]>,
    element: &BytesStart<'_>,
    key: &[u8],
) -> Result<StyleAttribute<u32>, String> {
    let Some(value) = xml_attribute(reader, element, key)? else {
        return Ok(StyleAttribute::Missing);
    };
    Ok(value
        .parse::<u32>()
        .map_or(StyleAttribute::Invalid, StyleAttribute::Value))
}

fn style_f64_attribute(
    reader: &XmlReader<&[u8]>,
    element: &BytesStart<'_>,
    key: &[u8],
) -> Result<StyleAttribute<f64>, String> {
    let Some(value) = xml_attribute(reader, element, key)? else {
        return Ok(StyleAttribute::Missing);
    };
    Ok(value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map_or(StyleAttribute::Invalid, StyleAttribute::Value))
}

fn style_on_off_attribute(
    reader: &XmlReader<&[u8]>,
    element: &BytesStart<'_>,
) -> Result<StyleAttribute<bool>, String> {
    let Some(value) = xml_attribute(reader, element, b"val")? else {
        return Ok(StyleAttribute::Value(true));
    };
    Ok(match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "on" => StyleAttribute::Value(true),
        "0" | "false" | "off" => StyleAttribute::Value(false),
        _ => StyleAttribute::Invalid,
    })
}

fn style_underline_attribute(
    reader: &XmlReader<&[u8]>,
    element: &BytesStart<'_>,
) -> Result<StyleAttribute<bool>, String> {
    let Some(value) = xml_attribute(reader, element, b"val")? else {
        return Ok(StyleAttribute::Value(true));
    };
    Ok(match value.as_str() {
        "single" | "double" | "singleAccounting" | "doubleAccounting" => {
            StyleAttribute::Value(true)
        }
        "none" => StyleAttribute::Value(false),
        _ => StyleAttribute::Invalid,
    })
}

fn xml_u32_attribute(
    reader: &XmlReader<&[u8]>,
    element: &BytesStart<'_>,
    key: &[u8],
    description: &str,
) -> Result<Option<u32>, String> {
    xml_attribute(reader, element, key)?
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|_| format!("invalid {description} {value:?}"))
        })
        .transpose()
}

fn xml_f64_attribute(
    reader: &XmlReader<&[u8]>,
    element: &BytesStart<'_>,
    key: &[u8],
    description: &str,
) -> Result<Option<f64>, String> {
    xml_attribute(reader, element, key)?
        .map(|value| {
            value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| format!("invalid {description} {value:?}"))
        })
        .transpose()
}

struct ParsedSheetMetadata {
    cells: HashMap<(u32, u32), CellMetadata>,
    columns: Result<Vec<ColumnDeclaration>, String>,
}

fn parse_sheet_metadata(xml: &[u8]) -> Result<ParsedSheetMetadata, String> {
    let mut reader = XmlReader::from_reader(xml);
    let mut buffer = Vec::new();
    let mut cells = HashMap::new();
    let mut columns = Vec::new();
    let mut column_error = None;
    let mut in_columns = false;
    let mut row_index = 0_u32;
    let mut col_index = 0_u32;

    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) if element.local_name().as_ref() == b"cols" => {
                in_columns = true;
            }
            Ok(Event::End(element)) if element.local_name().as_ref() == b"cols" => {
                in_columns = false;
            }
            Ok(Event::Start(element) | Event::Empty(element))
                if in_columns && element.local_name().as_ref() == b"col" =>
            {
                if column_error.is_none() {
                    match parse_column_declaration(&reader, &element) {
                        Ok(Some(declaration)) => columns.push(declaration),
                        Ok(None) => {}
                        Err(message) => column_error = Some(message),
                    }
                }
            }
            Ok(Event::Start(element)) if element.local_name().as_ref() == b"row" => {
                if let Some(row) =
                    xml_attribute(&reader, &element, b"r")?.and_then(|row| row.parse::<u32>().ok())
                {
                    row_index = row.saturating_sub(1);
                }
            }
            Ok(Event::End(element)) if element.local_name().as_ref() == b"row" => {
                row_index = row_index.saturating_add(1);
                col_index = 0;
            }
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"c" =>
            {
                let position = xml_attribute(&reader, &element, b"r")?
                    .and_then(|reference| parse_a1_cell(&reference))
                    .unwrap_or((row_index, col_index));
                col_index = position.1.saturating_add(1);
                let style_index = xml_attribute(&reader, &element, b"s")?
                    .and_then(|style| style.parse::<usize>().ok());
                let declared_type = xml_attribute(&reader, &element, b"t")?
                    .as_deref()
                    .and_then(declared_cell_type);
                cells.insert(
                    position,
                    CellMetadata {
                        style_index,
                        declared_type,
                    },
                );
            }
            Ok(Event::Eof) => {
                if in_columns && column_error.is_none() {
                    column_error = Some("invalid worksheet XML: unclosed cols element".to_owned());
                }
                break;
            }
            Ok(_) => {}
            Err(error) => return Err(format!("invalid worksheet XML: {error}")),
        }
    }
    Ok(ParsedSheetMetadata {
        cells,
        columns: column_error.map_or_else(|| Ok(columns), Err),
    })
}

#[cfg(test)]
fn parse_column_declarations(xml: &[u8]) -> Result<Vec<ColumnDeclaration>, String> {
    parse_sheet_metadata(xml)?.columns
}

fn parse_column_declaration(
    reader: &XmlReader<&[u8]>,
    element: &BytesStart<'_>,
) -> Result<Option<ColumnDeclaration>, String> {
    let custom_width = match xml_attribute(reader, element, b"customWidth")? {
        Some(value) if matches!(value.as_str(), "1" | "true" | "on") => true,
        Some(value) if matches!(value.as_str(), "0" | "false" | "off") => false,
        Some(value) => return Err(format!("invalid customWidth {value:?}")),
        None => false,
    };
    if !custom_width {
        return Ok(None);
    }
    let min = xml_u32_attribute(reader, element, b"min", "column min")?
        .ok_or_else(|| "custom-width column has no min".to_owned())?;
    let max = xml_u32_attribute(reader, element, b"max", "column max")?
        .ok_or_else(|| "custom-width column has no max".to_owned())?;
    let width = xml_f64_attribute(reader, element, b"width", "column width")?
        .ok_or_else(|| "custom-width column has no width".to_owned())?;
    if min == 0 || max < min || width < 0.0 {
        return Err(format!(
            "invalid custom-width column min={min} max={max} width={width}"
        ));
    }
    Ok(Some(ColumnDeclaration { min, max, width }))
}

fn xml_attribute(
    reader: &XmlReader<&[u8]>,
    element: &BytesStart<'_>,
    key: &[u8],
) -> Result<Option<String>, String> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| format!("invalid XML attribute: {error}"))?;
        if attribute.key.as_ref() == key {
            return attribute
                .decode_and_unescape_value(reader.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(|error| format!("invalid XML attribute value: {error}"));
        }
    }
    Ok(None)
}

fn xml_attribute_by_local_name(
    reader: &XmlReader<&[u8]>,
    element: &BytesStart<'_>,
    key: &[u8],
) -> Result<Option<String>, String> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| format!("invalid XML attribute: {error}"))?;
        if attribute.key.local_name().as_ref() == key {
            return attribute
                .decode_and_unescape_value(reader.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(|error| format!("invalid XML attribute value: {error}"));
        }
    }
    Ok(None)
}

fn declared_cell_type(value: &str) -> Option<CellType> {
    match value {
        "n" => Some(CellType::N),
        "s" | "str" | "inlineStr" => Some(CellType::S),
        "b" => Some(CellType::B),
        "e" => Some(CellType::E),
        "d" => Some(CellType::D),
        _ => None,
    }
}

fn parse_a1_cell(reference: &str) -> Option<(u32, u32)> {
    let reference = reference.trim_matches('$');
    let split = reference.find(|character: char| character.is_ascii_digit())?;
    let (column, row) = reference.split_at(split);
    let row = row.parse::<u32>().ok()?.checked_sub(1)?;
    let mut column_number = 0_u32;
    for byte in column.bytes() {
        let upper = byte.to_ascii_uppercase();
        if !upper.is_ascii_uppercase() {
            return None;
        }
        column_number = column_number
            .checked_mul(26)?
            .checked_add(u32::from(upper - b'A' + 1))?;
    }
    Some((row, column_number.checked_sub(1)?))
}

fn relationships_path(part: &str) -> String {
    let (directory, file) = part
        .rsplit_once('/')
        .map_or(("", part), |(directory, file)| (directory, file));
    if directory.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{directory}/_rels/{file}.rels")
    }
}

fn resolve_part(base_part: &str, target: &str) -> String {
    if target.starts_with('/') {
        return normalize_part(target);
    }
    let base_directory = base_part
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    normalize_part(&format!("{base_directory}/{target}"))
}

fn normalize_part(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut components = Vec::new();
    for component in path.trim_start_matches('/').split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            component => components.push(component),
        }
    }
    components.join("/")
}

// ECMA-376 legacy indexed palette (18.8.27). The leading alpha byte from
// the source table is deliberately omitted because the wax model stores
// opaque #RRGGBB colors. Indices 64 and 65 are system colors and cannot be
// resolved portably.
const LEGACY_INDEXED_COLORS: [&str; 64] = [
    "000000", "FFFFFF", "FF0000", "00FF00", "0000FF", "FFFF00", "FF00FF", "00FFFF", "000000",
    "FFFFFF", "FF0000", "00FF00", "0000FF", "FFFF00", "FF00FF", "00FFFF", "800000", "008000",
    "000080", "808000", "800080", "008080", "C0C0C0", "808080", "9999FF", "993366", "FFFFCC",
    "CCFFFF", "660066", "FF8080", "0066CC", "CCCCFF", "000080", "FF00FF", "FFFF00", "00FFFF",
    "800080", "800000", "008080", "0000FF", "00CCFF", "CCFFFF", "CCFFCC", "FFFF99", "99CCFF",
    "FF99CC", "CC99FF", "FFCC99", "3366FF", "33CCCC", "99CC00", "FFCC00", "FF9900", "FF6600",
    "666699", "969696", "003366", "339966", "003300", "333300", "993300", "993366", "333399",
    "333333",
];

// ECMA-376 built-in number formats 0-49. IDs 23-36 are reserved for
// locale-dependent formats and therefore deliberately resolve to unknown.
const BUILTIN_FORMATS: [Option<&str>; 50] = [
    Some("General"),
    Some("0"),
    Some("0.00"),
    Some("#,##0"),
    Some("#,##0.00"),
    Some(r##""$"#,##0_);("$"#,##0)"##),
    Some(r##""$"#,##0_);[Red]("$"#,##0)"##),
    Some(r##""$"#,##0.00_);("$"#,##0.00)"##),
    Some(r##""$"#,##0.00_);[Red]("$"#,##0.00)"##),
    Some("0%"),
    Some("0.00%"),
    Some("0.00E+00"),
    Some("# ?/?"),
    Some("# ??/??"),
    Some("mm-dd-yy"),
    Some("d-mmm-yy"),
    Some("d-mmm"),
    Some("mmm-yy"),
    Some("h:mm AM/PM"),
    Some("h:mm:ss AM/PM"),
    Some("h:mm"),
    Some("h:mm:ss"),
    Some("m/d/yy h:mm"),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    Some("#,##0 ;(#,##0)"),
    Some("#,##0 ;[Red](#,##0)"),
    Some("#,##0.00;(#,##0.00)"),
    Some("#,##0.00;[Red](#,##0.00)"),
    Some(r#"_(* #,##0_);_(* \(#,##0\);_(* "-"_);_(@_)"#),
    Some(r#"_("$"* #,##0_);_("$"* \(#,##0\);_("$"* "-"_);_(@_)"#),
    Some(r#"_(* #,##0.00_);_(* \(#,##0.00\);_(* "-"??_);_(@_)"#),
    Some(r#"_("$"* #,##0.00_);_("$"* \(#,##0.00\);_("$"* "-"??_);_(@_)"#),
    Some("mm:ss"),
    Some("[h]:mm:ss"),
    Some("mmss.0"),
    Some("##0.0E+0"),
    Some("@"),
];

fn builtin_format(id: u32) -> Option<&'static str> {
    BUILTIN_FORMATS.get(id as usize).copied().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_format_table_covers_zero_through_forty_nine() {
        assert_eq!(BUILTIN_FORMATS.len(), 50);
        assert_eq!(builtin_format(0), Some("General"));
        assert_eq!(builtin_format(14), Some("mm-dd-yy"));
        assert_eq!(builtin_format(23), None);
        assert_eq!(builtin_format(49), Some("@"));
        assert_eq!(builtin_format(50), None);
    }

    #[test]
    fn structural_legacy_xls_errors_are_bad_zip_not_internal() {
        for message in [
            "Invalid short string length, expected at least 2, found 1",
            "Cfb error: Sector 4294967274 points past end of file",
        ] {
            let failure = container_error(WorkbookKind::Xls, message);
            assert_eq!(failure.code, ErrorCode::BadZip, "{message}");
        }
    }

    #[test]
    fn explicit_empty_string_cells_are_not_dropped() {
        let cell = normalize_cell(
            (2, 3),
            &Data::String(String::new()),
            None,
            None,
            None,
            false,
        )
        .expect("an explicit empty string is still a cell");

        assert_eq!(cell.t, CellType::S);
        assert_eq!(cell.v, Some(CellValue::Text(String::new())));
        assert_eq!(cell.d, Some(String::new()));
    }

    #[test]
    fn column_widths_expand_ranges_honor_custom_width_and_cap_whole_sheet_spans() {
        let xml = br#"
            <worksheet>
              <cols>
                <col min="1" max="1" width="12.5" customWidth="1"/>
                <col min="3" max="5" width="24" customWidth="true"/>
                <col min="6" max="6" width="99" customWidth="false"/>
                <col min="1" max="16384" width="8.25" customWidth="1"/>
              </cols>
              <sheetData/>
            </worksheet>
        "#;
        let columns = parse_column_declarations(xml).expect("column declarations should parse");
        assert_eq!(columns.len(), 3);
        assert_eq!(
            columns[0],
            ColumnDeclaration {
                min: 1,
                max: 1,
                width: 12.5
            }
        );
        assert_eq!(
            columns[1],
            ColumnDeclaration {
                min: 3,
                max: 5,
                width: 24.0
            }
        );

        let supplement = OoxmlSupplement {
            sheets: HashMap::from([(
                "Sheet1".to_owned(),
                OoxmlSheetSupplement {
                    cells: HashMap::new(),
                    columns,
                },
            )]),
            ..OoxmlSupplement::default()
        };
        let widths = supplement.col_infos("Sheet1", 2);
        assert_eq!(widths.len(), 258);
        assert_eq!(widths[0], ColInfo { c: 0, width: 8.25 });
        assert_eq!(
            widths[257],
            ColInfo {
                c: 257,
                width: 8.25
            }
        );
        assert!(!widths.iter().any(|info| info.width == 99.0));
    }

    #[test]
    fn overlapping_whole_sheet_widths_are_bounded_by_declarations_plus_cap() {
        let columns = (0..20_000)
            .map(|index| ColumnDeclaration {
                min: 1,
                max: 16_384,
                width: f64::from(index),
            })
            .collect();
        let supplement = OoxmlSupplement {
            sheets: HashMap::from([(
                "Sheet1".to_owned(),
                OoxmlSheetSupplement {
                    cells: HashMap::new(),
                    columns,
                },
            )]),
            ..OoxmlSupplement::default()
        };

        let started = Instant::now();
        let widths = supplement.col_infos("Sheet1", 16_384);
        let elapsed = started.elapsed();

        assert_eq!(widths.len(), 16_384);
        assert_eq!(
            widths[0],
            ColInfo {
                c: 0,
                width: 19_999.0
            }
        );
        assert_eq!(
            widths[16_383],
            ColInfo {
                c: 16_383,
                width: 19_999.0
            }
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "20,000 overlapping declarations took {elapsed:?}"
        );
    }

    #[test]
    fn parses_basic_fonts_solid_fills_and_supported_colors() {
        let xml = br##"
            <styleSheet>
              <fonts count="4">
                <font/>
                <font>
                  <b/><i/><u val="double"/><strike/>
                  <sz val="13.5"/><name val="Aptos"/>
                  <color rgb="801a2b3c"/>
                </font>
                <font><color indexed="55"/></font>
                <font><color theme="3" tint="-0.25"/></font>
              </fonts>
              <fills count="4">
                <fill><patternFill patternType="none"/></fill>
                <fill><patternFill patternType="solid"><fgColor rgb="FFEEDDCC"/></patternFill></fill>
                <fill><patternFill patternType="solid"><fgColor indexed="24"/></patternFill></fill>
                <fill><patternFill patternType="solid"><fgColor theme="4"/></patternFill></fill>
              </fills>
              <cellXfs count="5">
                <xf numFmtId="0" fontId="0" fillId="0"/>
                <xf numFmtId="4" fontId="1" fillId="1"/>
                <xf numFmtId="0" fontId="2" fillId="2"/>
                <xf numFmtId="0" fontId="3" fillId="3"/>
                <xf numFmtId="0" fontId="0" fillId="0"/>
              </cellXfs>
            </styleSheet>
        "##;
        let xfs = parse_styles(xml).expect("styles should parse");
        assert_eq!(xfs.len(), 5);
        assert_eq!(xfs[0].style, None);
        assert_eq!(xfs[0].format, None);

        let first = xfs[1].style.as_ref().expect("first style");
        assert!(first.bold);
        assert!(first.italic);
        assert!(first.underline);
        assert!(first.strike);
        assert_eq!(first.font_size, Some(13.5));
        assert_eq!(first.font_name.as_deref(), Some("Aptos"));
        assert_eq!(first.font_color.as_deref(), Some("#1A2B3C"));
        assert_eq!(first.fill_color.as_deref(), Some("#EEDDCC"));
        assert_eq!(xfs[1].format.as_deref(), Some("#,##0.00"));

        let indexed = xfs[2].style.as_ref().expect("indexed style");
        assert_eq!(indexed.font_color.as_deref(), Some("#969696"));
        assert_eq!(indexed.fill_color.as_deref(), Some("#9999FF"));
        assert_eq!(xfs[3].style, None, "theme and tint colors are dropped");
        assert_eq!(xfs[4].style, None, "fully-default XF has no style");
    }

    #[test]
    fn malformed_style_entries_keep_xf_alignment_and_valid_neighbors() {
        let xml = br##"
            <styleSheet>
              <numFmts count="2">
                <numFmt numFmtId="164" formatCode="yyyy-mm-dd"/>
                <numFmt numFmtId="bad" formatCode="discarded"/>
              </numFmts>
              <fonts count="3">
                <font><b/></font>
                <font><b/><sz val="large"/><color rgb="auto"/></font>
                <font><i/><color rgb="FF112233"/></font>
              </fonts>
              <fills count="3">
                <fill/>
                <fill><patternFill patternType="solid"><fgColor rgb="auto"/></patternFill></fill>
                <fill><patternFill patternType="solid"><fgColor rgb="FF445566"/></patternFill></fill>
              </fills>
              <cellXfs count="5">
                <xf numFmtId="164" fontId="0" fillId="0"/>
                <xf numFmtId="4" fontId="1" fillId="1"/>
                <xf numFmtId="bad" fontId="2" fillId="2"/>
                <xf numFmtId="14" fontId="bad" fillId="2"/>
                <xf numFmtId="14" fontId="2" fillId="2"/>
              </cellXfs>
            </styleSheet>
        "##;

        let xfs = parse_styles(xml).expect("attribute junk should stay local to its entry");

        assert_eq!(xfs.len(), 5, "bad entries must retain XF indexes");
        assert_eq!(xfs[0].format.as_deref(), Some("yyyy-mm-dd"));
        assert!(xfs[0].style.as_ref().is_some_and(|style| style.bold));
        assert_eq!(xfs[1].format.as_deref(), Some("#,##0.00"));
        assert_eq!(xfs[1].style, None, "bad font/fill become placeholders");
        assert_eq!(xfs[2].format, None, "bad numFmtId skips one XF");
        assert_eq!(xfs[2].style, None, "bad numFmtId skips one XF");
        assert_eq!(xfs[3].format, None, "bad fontId skips one XF");
        assert_eq!(xfs[3].style, None, "bad fontId skips one XF");
        assert_eq!(xfs[4].format.as_deref(), Some("mm-dd-yy"));
        assert_eq!(
            xfs[4].style,
            Some(CellStyle {
                italic: true,
                font_color: Some("#112233".to_owned()),
                fill_color: Some("#445566".to_owned()),
                ..CellStyle::default()
            })
        );
    }

    #[test]
    fn style_interning_deduplicates_and_never_interns_the_default() {
        let style = CellStyle {
            bold: true,
            font_color: Some("#112233".to_owned()),
            ..CellStyle::default()
        };
        let mut styles = Vec::new();
        assert_eq!(intern_style(&mut styles, &style), Some(0));
        assert_eq!(intern_style(&mut styles, &style), Some(0));
        assert_eq!(intern_style(&mut styles, &CellStyle::default()), None);
        assert_eq!(styles, [style]);
    }

    #[test]
    fn missing_cell_style_attribute_resolves_xf_zero() {
        let base_style = CellStyle {
            font_size: Some(11.0),
            font_name: Some("Aptos".to_owned()),
            ..CellStyle::default()
        };
        let supplement = OoxmlSupplement {
            sheets: HashMap::from([(
                "Sheet1".to_owned(),
                OoxmlSheetSupplement {
                    cells: HashMap::from([(
                        (0, 0),
                        CellMetadata {
                            style_index: None,
                            declared_type: Some(CellType::S),
                        },
                    )]),
                    columns: Vec::new(),
                },
            )]),
            xfs: vec![ResolvedXf {
                format: None,
                style: Some(base_style.clone()),
            }],
            warnings: Vec::new(),
        };
        let metadata = supplement.cell("Sheet1", (0, 0)).expect("cell metadata");
        assert_eq!(metadata.style, Some(&base_style));
        assert_eq!(metadata.declared_type, Some(CellType::S));
    }

    #[test]
    fn malformed_xml_and_columns_are_rejected_for_fail_soft_callers() {
        assert!(parse_styles(b"<styleSheet><fonts>").is_err());
        assert!(parse_column_declarations(
            br#"<worksheet><cols><col min="3" max="1" width="8" customWidth="1"/></cols></worksheet>"#
        )
        .is_err());
    }

    #[test]
    fn a1_addresses_round_trip() {
        for (position, address) in [
            ((0, 0), "A1"),
            ((0, 25), "Z1"),
            ((1, 26), "AA2"),
            ((99, 16_383), "XFD100"),
        ] {
            assert_eq!(a1_cell(position), address);
            assert_eq!(parse_a1_cell(address), Some(position));
        }
    }

    #[test]
    fn formulas_drop_only_container_prefixes() {
        assert_eq!(
            normalize_formula("=SUM(A1:A2)".to_owned()).as_deref(),
            Some("SUM(A1:A2)")
        );
        assert_eq!(
            normalize_formula("of:=SUM([.A1:.A2])".to_owned()).as_deref(),
            Some("SUM([.A1:.A2])")
        );
        assert_eq!(normalize_formula("".to_owned()), None);
    }

    #[test]
    fn shared_formula_expansion_uses_the_physical_anchor_cell() {
        let mut shared = HashMap::new();
        let anchor = normalize_xlsx_formula(
            Some(XlsxFormulaMetadata::Shared {
                shared_index: 7,
                range: Some(Dimensions::new((10, 0), (10, 3))),
                formula: "$A11*C$2".to_owned(),
            }),
            (10, 2),
            &mut shared,
        );
        assert_eq!(anchor.as_deref(), Some("$A11*C$2"));

        let derived = normalize_xlsx_formula(
            Some(XlsxFormulaMetadata::SharedDerived { shared_index: 7 }),
            (10, 3),
            &mut shared,
        );
        assert_eq!(derived.as_deref(), Some("$A11*D$2"));
    }

    #[test]
    fn excel_dates_keep_millisecond_precision() {
        let midnight =
            calamine::ExcelDateTime::new(0.0, calamine::ExcelDateTimeType::DateTime, true);
        assert_eq!(excel_datetime_iso(midnight), "1904-01-01");

        let timestamp = calamine::ExcelDateTime::new(
            0.500_001_423_611_111,
            calamine::ExcelDateTimeType::DateTime,
            true,
        );
        assert_eq!(excel_datetime_iso(timestamp), "1904-01-01T12:00:00.123");
    }

    #[test]
    fn non_finite_literal_numbers_are_not_emitted() {
        assert_eq!(
            normalize_cell((0, 0), &Data::Float(f64::NAN), None, None, None, false),
            None
        );
        let formula = normalize_cell(
            (0, 0),
            &Data::Float(f64::NAN),
            Some("0/0".to_owned()),
            None,
            Some(CellType::N),
            false,
        )
        .expect("formula cells remain present when their cache is invalid");
        assert_eq!(formula.v, None);
    }

    #[test]
    fn supplemental_format_codes_classify_dates_but_not_durations() {
        assert_eq!(number_format_kind("mm/dd/yyyy"), NumberFormatKind::DateTime);
        assert_eq!(number_format_kind("h:mm AM/PM"), NumberFormatKind::DateTime);
        assert_eq!(number_format_kind("[hh]:mm:ss"), NumberFormatKind::Duration);
        assert_eq!(
            number_format_kind(r#""M" #,##0.00"#),
            NumberFormatKind::Other
        );
        assert_eq!(number_format_kind("#,##0"), NumberFormatKind::Other);

        assert_eq!(
            normalize_number_value(39_638.0, "mm/dd/yyyy", false),
            (CellType::D, CellValue::Text("2008-07-09".to_owned()))
        );
        assert_eq!(
            normalize_number_value(0.9, "[h]", false),
            (CellType::N, CellValue::Number(0.9))
        );
        assert_eq!(
            normalize_number_value(-12_345.678_9, "mm-dd-yy", false),
            (CellType::N, CellValue::Number(-12_345.678_9))
        );
    }

    #[test]
    fn calamine_datetime_typing_survives_a_missing_format_supplement() {
        // A degraded/absent styles supplement leaves format=None; calamine's
        // own datetime typing must still win for non-negative serials.
        let value =
            calamine::ExcelDateTime::new(25_569.5, calamine::ExcelDateTimeType::DateTime, false);
        let cell = normalize_cell((0, 0), &Data::DateTime(value), None, None, None, false)
            .expect("datetime cell");
        assert_eq!(cell.t, CellType::D);
        assert_eq!(
            cell.v,
            Some(CellValue::Text("1970-01-01T12:00:00".to_owned()))
        );

        let negative =
            calamine::ExcelDateTime::new(-1.25, calamine::ExcelDateTimeType::DateTime, false);
        let cell = normalize_cell((0, 0), &Data::DateTime(negative), None, None, None, false)
            .expect("negative serial cell");
        assert_eq!(cell.t, CellType::N);
        assert_eq!(cell.v, Some(CellValue::Number(-1.25)));
    }
}

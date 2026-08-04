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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader as XmlReader, Writer as XmlWriter};
use rust_xlsxwriter::{
    Color, ExcelDateTime, Format, FormatUnderline, Formula, Workbook, Worksheet, XlsxError,
};
use tempfile::{Builder as TempFileBuilder, NamedTempFile};
use wax_core::{CellOverride, CellStyle, CellType, CellValue, SizeOverrides, EXPORT_OVERRIDES_CAP};
use wax_eval::{CellRef as EvalCellRef, EvaluatedCell};
use wax_fmt::FmtValue;
use wax_store::{WindowCell, WorkbookStore};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

const CSV_DROPPED: [&str; 6] = [
    "formulas (cached values only)",
    "number formatting beyond display strings",
    "merges",
    "styles",
    "column widths",
    "row heights",
];
const DEFAULT_DATE_FORMAT: &str = "yyyy-mm-dd";
const DEFAULT_DATETIME_FORMAT: &str = r#"yyyy-mm-dd"T"hh:mm:ss"#;
const DEFAULT_DATETIME_MILLIS_FORMAT: &str = r#"yyyy-mm-dd"T"hh:mm:ss.000"#;
const DEFAULT_TIME_FORMAT: &str = "hh:mm:ss";
const DEFAULT_TIME_MILLIS_FORMAT: &str = "hh:mm:ss.000";
const UNREPRESENTABLE_MERGES_DROPPED: &str = "unrepresentable merge ranges";
const CLAMPED_COLUMN_WIDTHS_DROPPED: &str = "column widths clamped to 0..=255";
const CLAMPED_ROW_HEIGHTS_DROPPED: &str = "row heights clamped to 0..=409.5";
const XLSX_MAX_STRING_CHARS: usize = 32_767;
const MAX_DROPPED_DETAILS: usize = 100;
const XLSX_MAX_ROWS: u32 = 1_048_576;
const XLSX_MAX_COLS: u32 = 16_384;
/// Cell cap for override-extended sheet extents, mirroring the reader's
/// declared-extent bomb rail (`ReaderOptions::max_declared_cells`).
const OVERRIDE_EXTENT_CAP_CELLS: u64 = 8_000_000;

/// A successful export: bytes written to the output file plus every feature
/// of the model (or of the source, when the caller merges open-time
/// warnings) that the export does not preserve. `dropped` entries are short
/// human-readable phrases, e.g. `"pivot caches"`, `"cell borders"`.
///
/// `applied` counts the post-collapse distinct override cells applied to the
/// exported sheet(s) — duplicates are last-wins (amendment A4) and, for CSV,
/// only the exported sheet counts (amendment A6). Clearing an already-empty
/// cell still counts: the override was accepted and the result matches the
/// request. Zero when the export carried no overrides.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportOutcome {
    pub bytes: u64,
    pub applied: u64,
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

/// Overrides collapsed per sheet: last-wins merged values keyed by
/// coordinate (amendment A4), plus the maximum targeted row/column for
/// recomputing sheet dims as `max(store dims, override max + 1)`.
#[derive(Debug, Default)]
struct SheetOverrides {
    cells: HashMap<(u32, u32), Option<CellValue>>,
    max_row: u32,
    max_col: u32,
}

/// Validate and collapse export overrides (v0.2 contract). `scope` limits
/// the collapse to one sheet for CSV (amendment A6: overrides targeting
/// other sheets are ignored, not rejected) — but an out-of-range sheet
/// index is `bad_request` in every format, never a silent skip (A5), and
/// extent growth breaching the cell cap is `bomb` (A5).
fn collapse_overrides(
    store: &WorkbookStore,
    overrides: &[CellOverride],
    scope: Option<u32>,
) -> Result<HashMap<u32, SheetOverrides>, WriteError> {
    if overrides.len() > EXPORT_OVERRIDES_CAP {
        return Err(WriteError::new(
            "bad_request",
            format!(
                "overrides length {} exceeds the {EXPORT_OVERRIDES_CAP}-entry cap",
                overrides.len()
            ),
        ));
    }
    let sheet_count = store.sheet_count();
    let mut collapsed: HashMap<u32, SheetOverrides> = HashMap::new();
    for (index, entry) in overrides.iter().enumerate() {
        if entry.sheet >= sheet_count {
            return Err(WriteError::new(
                "bad_request",
                format!(
                    "overrides[{index}] sheet index {} is out of range",
                    entry.sheet
                ),
            ));
        }
        if let Some(CellValue::Number(number)) = &entry.v {
            if !number.is_finite() {
                return Err(WriteError::new(
                    "bad_request",
                    format!("overrides[{index}].v must be a finite number"),
                ));
            }
        }
        if scope.is_some_and(|sheet| sheet != entry.sheet) {
            continue;
        }
        let sheet = collapsed.entry(entry.sheet).or_default();
        sheet.max_row = sheet.max_row.max(entry.r);
        sheet.max_col = sheet.max_col.max(entry.c);
        sheet.cells.insert((entry.r, entry.c), entry.v.clone());
    }
    for (sheet_index, sheet) in &collapsed {
        let meta = store.sheet_meta(*sheet_index).ok_or_else(|| {
            WriteError::new(
                "internal",
                format!("sheet index {sheet_index} disappeared during override collapse"),
            )
        })?;
        let rows = meta.rows.max(sheet.max_row.saturating_add(1));
        let cols = meta.cols.max(sheet.max_col.saturating_add(1));
        let original = u64::from(meta.rows) * u64::from(meta.cols);
        let extended = u64::from(rows) * u64::from(cols);
        if extended > OVERRIDE_EXTENT_CAP_CELLS && extended > original {
            return Err(WriteError::new(
                "bomb",
                format!(
                    "overrides extend sheet {name:?} to {rows}x{cols} ({extended} cells), \
                     exceeding the {OVERRIDE_EXTENT_CAP_CELLS} cell limit",
                    name = meta.name
                ),
            ));
        }
    }
    Ok(collapsed)
}

/// Build the cell an override produces (amendments A1–A3): the value is
/// replaced, the display string is recomputed by re-rendering the retained
/// format code through wax-fmt (`None` when nothing is retained, so CSV
/// falls back to raw), the style id and format code of an overridden cell
/// are kept, a formula is always dropped, strings are never reinterpreted
/// as formulas, and numbers are never coerced to dates. A synthesized cell
/// (`stored: None`) retains nothing and lands on the XF-0 base style.
///
/// Display recompute uses the 1900 epoch: the exported workbook is always
/// 1900-based (date cells travel as ISO text through `ExcelDateTime`), so
/// this matches what wax's own reader shows on read-back of the export.
fn overridden_cell(stored: Option<&WindowCell>, value: Option<&CellValue>) -> WindowCell {
    let fmt = stored.and_then(|cell| cell.fmt.clone());
    let (t, d) = match value {
        None => (CellType::S, None),
        Some(CellValue::Number(number)) => (
            CellType::N,
            render_override_display(fmt.as_deref(), FmtValue::Number(*number)),
        ),
        Some(CellValue::Text(text)) => (
            CellType::S,
            render_override_display(fmt.as_deref(), FmtValue::Text(text)),
        ),
        Some(CellValue::Bool(boolean)) => (
            CellType::B,
            render_override_display(fmt.as_deref(), FmtValue::Bool(*boolean)),
        ),
    };
    WindowCell {
        t,
        v: value.cloned(),
        d,
        f: None,
        fmt,
        e: false,
        s: stored.and_then(|cell| cell.s),
    }
}

fn render_override_display(fmt: Option<&str>, value: FmtValue<'_>) -> Option<String> {
    fmt.and_then(|code| wax_fmt::render(code, value, false))
}

/// Write the whole workbook as a styled xlsx copy: values, types, formula
/// text with cached results, number formats, merges, explicit column widths,
/// and basic cell styles. `cancel` is checked at row-granularity
/// checkpoints; a cancelled export returns `code: "cancelled"` and leaves no
/// partial output file behind.
///
/// Per-sheet last-wins size edits, validated against the store.
#[derive(Clone, Debug, Default)]
struct SheetSizeOverrides {
    cols: HashMap<u32, f64>,
    rows: HashMap<u32, f64>,
}

/// Validate and collapse export size overrides (v0.3 exportSizeOverrides):
/// the combined count shares the cell-override cap, sheet indices must
/// exist, and targets must lie inside the xlsx grid. Values are collapsed
/// last-wins per column/row; out-of-range sizes are clamped loudly at
/// application time, not rejected here.
fn collapse_size_overrides(
    store: &WorkbookStore,
    sizes: &SizeOverrides,
) -> Result<HashMap<u32, SheetSizeOverrides>, WriteError> {
    if sizes.len() > EXPORT_OVERRIDES_CAP {
        return Err(WriteError::new(
            "bad_request",
            format!(
                "sizeOverrides length {} exceeds the {EXPORT_OVERRIDES_CAP}-entry cap",
                sizes.len()
            ),
        ));
    }
    let sheet_count = store.sheet_count();
    let mut collapsed: HashMap<u32, SheetSizeOverrides> = HashMap::new();
    for (index, entry) in sizes.cols.iter().enumerate() {
        if entry.sheet >= sheet_count {
            return Err(WriteError::new(
                "bad_request",
                format!(
                    "sizeOverrides.cols[{index}] sheet index {} is out of range",
                    entry.sheet
                ),
            ));
        }
        if entry.c >= XLSX_MAX_COLS {
            return Err(WriteError::new(
                "bad_request",
                format!(
                    "sizeOverrides.cols[{index}] column {} exceeds the xlsx limit",
                    entry.c
                ),
            ));
        }
        collapsed
            .entry(entry.sheet)
            .or_default()
            .cols
            .insert(entry.c, entry.width);
    }
    for (index, entry) in sizes.rows.iter().enumerate() {
        if entry.sheet >= sheet_count {
            return Err(WriteError::new(
                "bad_request",
                format!(
                    "sizeOverrides.rows[{index}] sheet index {} is out of range",
                    entry.sheet
                ),
            ));
        }
        if entry.r >= XLSX_MAX_ROWS {
            return Err(WriteError::new(
                "bad_request",
                format!(
                    "sizeOverrides.rows[{index}] row {} exceeds the xlsx limit",
                    entry.r
                ),
            ));
        }
        collapsed
            .entry(entry.sheet)
            .or_default()
            .rows
            .insert(entry.r, entry.height);
    }
    Ok(collapsed)
}

pub fn write_xlsx(
    store: &WorkbookStore,
    out: &Path,
    cancel: &AtomicBool,
) -> Result<ExportOutcome, WriteError> {
    write_xlsx_with_overrides(store, out, &[], &SizeOverrides::default(), cancel)
}

/// [`write_xlsx`] with edited cell values layered over the read model per
/// the v0.2 export-with-overrides contract. The store is never mutated —
/// export stays side-effect-free; overrides are merged over the writer's
/// sheet scan, remaining out-of-extent overrides are synthesized as cells,
/// and sheet dims grow to `max(store dims, override max + 1)`.
pub fn write_xlsx_with_overrides(
    store: &WorkbookStore,
    out: &Path,
    overrides: &[CellOverride],
    sizes: &SizeOverrides,
    cancel: &AtomicBool,
) -> Result<ExportOutcome, WriteError> {
    write_xlsx_with_evaluated_overrides(store, out, overrides, sizes, &[], cancel)
}

/// v0.4 export path: evaluated formula values layer over file caches while
/// retaining each formula, format, and style. Literal cell overrides then
/// layer on top and keep the v0.2 formula-drop semantics.
pub fn write_xlsx_with_evaluated_overrides(
    store: &WorkbookStore,
    out: &Path,
    overrides: &[CellOverride],
    sizes: &SizeOverrides,
    evaluated: &[EvaluatedCell],
    cancel: &AtomicBool,
) -> Result<ExportOutcome, WriteError> {
    checkpoint(cancel)?;
    if store.sheet_count() == 0 {
        return Err(WriteError::new("bad_request", "empty workbook"));
    }
    let sheet_overrides = collapse_overrides(store, overrides, None)?;
    let sheet_size_overrides = collapse_size_overrides(store, sizes)?;
    let evaluated = evaluated
        .iter()
        .map(|cell| {
            (
                EvalCellRef {
                    sheet: cell.sheet,
                    r: cell.r,
                    c: cell.c,
                },
                cell,
            )
        })
        .collect::<HashMap<_, _>>();
    // The xlsx grid itself is an extent cap (A5: `too_large`). Checked after
    // collapse so the `bad_request` taxonomy (cap, unknown sheet) wins when
    // both apply.
    for (index, entry) in overrides.iter().enumerate() {
        if entry.r >= XLSX_MAX_ROWS {
            return Err(WriteError::new(
                "too_large",
                format!(
                    "overrides[{index}] targets row {}, exceeding the xlsx limit of \
                     {XLSX_MAX_ROWS} rows",
                    entry.r
                ),
            ));
        }
        if entry.c >= XLSX_MAX_COLS {
            return Err(WriteError::new(
                "too_large",
                format!(
                    "overrides[{index}] targets column {}, exceeding the xlsx limit of \
                     {XLSX_MAX_COLS} columns",
                    entry.c
                ),
            ));
        }
    }
    let applied = sheet_overrides
        .values()
        .map(|sheet| sheet.cells.len() as u64)
        .sum();
    let mut replaced_formula_cells = 0_usize;

    let mut workbook = Workbook::new();
    let mut formats = HashMap::new();
    let mut dropped = Dropped::default();
    let merge_format = Format::new();
    let mut sheet_patches = Vec::<SheetXmlPatch>::new();
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

        // Size overrides layer over the read model's declarations,
        // last-wins per column/row, before the shared clamp rail.
        let size_edits = sheet_size_overrides.get(&sheet_index);
        let mut effective_col_widths = store
            .sheet_col_infos(sheet_index)
            .expect("sheet metadata and column metadata must agree")
            .iter()
            .map(|info| (info.c, info.width))
            .collect::<BTreeMap<_, _>>();
        if let Some(edits) = size_edits {
            effective_col_widths.extend(edits.cols.iter().map(|(&c, &width)| (c, width)));
        }
        let mut effective_row_heights = store
            .sheet_row_infos(sheet_index)
            .expect("sheet metadata and row metadata must agree")
            .iter()
            .map(|info| (info.r, info.height))
            .collect::<BTreeMap<_, _>>();
        if let Some(edits) = size_edits {
            effective_row_heights.extend(edits.rows.iter().map(|(&r, &height)| (r, height)));
        }

        // set_column_width() creates the <col> elements (pixel-quantized);
        // the XML patch pass restores the exact character-unit values.
        let mut sheet_col_width_patches = HashMap::new();
        for (&column_index, &raw_width) in &effective_col_widths {
            checkpoint(cancel)?;
            let width = clamp_column_width(raw_width);
            if !raw_width.is_finite() || width != raw_width {
                dropped.add(CLAMPED_COLUMN_WIDTHS_DROPPED);
            }
            let column = xlsx_column(column_index)?;
            worksheet
                .set_column_width(column, width)
                .map_err(|error| xlsx_error("set column width", out, error))?;
            sheet_col_width_patches.insert(column_index.saturating_add(1), width);
        }

        // set_row_height() creates the <row> elements (pixel-quantized);
        // the XML patch pass restores the exact point values afterwards.
        let mut sheet_row_height_patches = HashMap::new();
        for (&row_index, &raw_height) in &effective_row_heights {
            checkpoint(cancel)?;
            let height = clamp_row_height(raw_height);
            if !raw_height.is_finite() || height != raw_height {
                dropped.add(CLAMPED_ROW_HEIGHTS_DROPPED);
            }
            let row = xlsx_row(row_index)?;
            worksheet
                .set_row_height(row, height)
                .map_err(|error| xlsx_error("set row height", out, error))?;
            sheet_row_height_patches.insert(row_index.saturating_add(1), height);
        }

        if let Some(default_height) = meta.default_row_height {
            let height = clamp_row_height(default_height);
            if !default_height.is_finite() || height != default_height {
                dropped.add(CLAMPED_ROW_HEIGHTS_DROPPED);
            }
            worksheet.set_default_row_height(height);
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

        let overrides_for_sheet = sheet_overrides.get(&sheet_index);
        let mut consumed_overrides = HashSet::new();
        let mut replaced_formulas = HashSet::new();
        let mut last_row = None;
        let mut scan_error = None;
        let mut sheet_formula_patches = Vec::new();
        let mut previous_position = None;
        let mut previous_had_formula = false;
        let scanned = store.scan_sheet(sheet_index, |row, column, mut stored_cell| {
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
            if let Some(value) = evaluated.get(&EvalCellRef {
                sheet: sheet_index,
                r: row,
                c: column,
            }) {
                value.apply(&mut stored_cell);
            }
            let cell = match overrides_for_sheet.and_then(|sheet| sheet.cells.get(&(row, column))) {
                Some(value) => {
                    consumed_overrides.insert((row, column));
                    if stored_cell.f.is_some() {
                        replaced_formulas.insert((row, column));
                    }
                    overridden_cell(Some(&stored_cell), value.as_ref())
                }
                None => stored_cell,
            };
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
        replaced_formula_cells += replaced_formulas.len();

        // Overrides not consumed by the scan target cells the store holds
        // nothing for (in-extent empties or beyond the extent): synthesize
        // them, sorted for determinism. Clearing an empty cell is a no-op.
        if let Some(sheet) = overrides_for_sheet {
            let mut remaining = sheet
                .cells
                .iter()
                .filter(|(position, _)| !consumed_overrides.contains(*position))
                .collect::<Vec<_>>();
            remaining.sort_by_key(|(position, _)| **position);
            for (&(row, column), value) in remaining {
                checkpoint(cancel)?;
                if value.is_none() {
                    continue;
                }
                let cell = overridden_cell(None, value.as_ref());
                write_xlsx_cell(
                    worksheet,
                    &mut formats,
                    store.styles(),
                    &meta.name,
                    row,
                    column,
                    &cell,
                    None,
                    &mut dropped,
                    out,
                )?;
            }
        }
        sheet_patches.push(SheetXmlPatch {
            formulas: sheet_formula_patches,
            row_heights: sheet_row_height_patches,
            col_widths: sheet_col_width_patches,
            default_row_height: meta.default_row_height.map(clamp_row_height),
            default_col_width: meta.default_col_width.map(clamp_column_width),
        });
    }

    if replaced_formula_cells > 0 {
        dropped.add(format!(
            "formulas replaced by edited values ({replaced_formula_cells})"
        ));
    }

    checkpoint(cancel)?;
    let mut temp = temporary_output(out, ".xlsx")?;
    workbook
        .save(temp.path())
        .map_err(|error| xlsx_error("save workbook", out, error))?;
    checkpoint(cancel)?;
    temp = normalize_generated_xml(temp, out, &sheet_patches, cancel)?;
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
        applied,
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
    write_csv_with_overrides(store, sheet, out, &[], &SizeOverrides::default(), cancel)
}

/// [`write_csv`] with edited cell values layered over the read model per
/// the v0.2 export-with-overrides contract. CSV exports one sheet, so
/// overrides targeting other (valid) sheets are ignored and `applied`
/// counts only the exported sheet (amendment A6). The store is never
/// mutated — export stays side-effect-free.
pub fn write_csv_with_overrides(
    store: &WorkbookStore,
    sheet: u32,
    out: &Path,
    overrides: &[CellOverride],
    sizes: &SizeOverrides,
    cancel: &AtomicBool,
) -> Result<ExportOutcome, WriteError> {
    write_csv_with_evaluated_overrides(store, sheet, out, overrides, sizes, &[], cancel)
}

/// CSV counterpart to [`write_xlsx_with_evaluated_overrides`]. Evaluated
/// display strings are used before literal overrides are layered.
pub fn write_csv_with_evaluated_overrides(
    store: &WorkbookStore,
    sheet: u32,
    out: &Path,
    overrides: &[CellOverride],
    sizes: &SizeOverrides,
    evaluated: &[EvaluatedCell],
    cancel: &AtomicBool,
) -> Result<ExportOutcome, WriteError> {
    let meta = store.sheet_meta(sheet).ok_or_else(|| {
        WriteError::new(
            "bad_request",
            format!("sheet index {sheet} is out of range"),
        )
    })?;
    checkpoint(cancel)?;
    let mut collapsed = collapse_overrides(store, overrides, Some(sheet))?;
    let sheet_overrides = collapsed.remove(&sheet).unwrap_or_default();
    let evaluated = evaluated
        .iter()
        .filter(|cell| cell.sheet == sheet)
        .map(|cell| ((cell.r, cell.c), cell))
        .collect::<HashMap<_, _>>();
    // Size overrides are validated with the full taxonomy but cannot be
    // represented in csv; the ones aimed at the exported sheet degrade
    // loudly below (A6 scope: other sheets' entries are ignored).
    let dropped_size_overrides = collapse_size_overrides(store, sizes)?
        .remove(&sheet)
        .map_or(0, |edits| edits.cols.len().saturating_add(edits.rows.len()));
    let applied = sheet_overrides.cells.len() as u64;
    let (out_rows, out_cols) = if sheet_overrides.cells.is_empty() {
        (meta.rows, meta.cols)
    } else {
        (
            meta.rows.max(sheet_overrides.max_row.saturating_add(1)),
            meta.cols.max(sheet_overrides.max_col.saturating_add(1)),
        )
    };

    // A1: every override starts as its synthesized rendering (no retained
    // format code, so `d` is null and CSV falls back to raw); the scan
    // upgrades entries whose position holds a stored cell with that cell's
    // retained format code and style.
    let mut override_rows: HashMap<u32, BTreeMap<u32, WindowCell>> = HashMap::new();
    for (&(row, column), value) in &sheet_overrides.cells {
        override_rows
            .entry(row)
            .or_default()
            .insert(column, overridden_cell(None, value.as_ref()));
    }
    let mut replaced_formulas = HashSet::new();

    let mut temp = temporary_output(out, ".csv")?;
    {
        let mut output = BufWriter::new(temp.as_file_mut());
        let mut next_output_row = 0_u32;
        let mut pending_row = None;
        let mut pending_cells = Vec::new();
        let mut scan_error = None;

        let scanned = store.scan_sheet(sheet, |row, column, mut cell| {
            if scan_error.is_some() {
                return;
            }

            if let Some(value) = evaluated.get(&(row, column)) {
                value.apply(&mut cell);
            }

            if pending_row != Some(row) {
                if let Some(previous_row) = pending_row.take() {
                    if let Err(error) = write_csv_row(
                        &mut output,
                        out_cols,
                        &pending_cells,
                        override_rows.get(&previous_row),
                        cancel,
                        out,
                    ) {
                        scan_error = Some(error);
                        return;
                    }
                    pending_cells.clear();
                    next_output_row = previous_row.saturating_add(1);
                }

                while next_output_row < row {
                    if let Err(error) = write_csv_row(
                        &mut output,
                        out_cols,
                        &[],
                        override_rows.get(&next_output_row),
                        cancel,
                        out,
                    ) {
                        scan_error = Some(error);
                        return;
                    }
                    next_output_row = next_output_row.saturating_add(1);
                }
                pending_row = Some(row);
            }

            if let Some(value) = sheet_overrides.cells.get(&(row, column)) {
                // The stored cell is superseded; upgrade the override's
                // rendering with its retained format code instead of
                // queueing it (last stored duplicate wins here too).
                if cell.f.is_some() {
                    replaced_formulas.insert((row, column));
                }
                override_rows
                    .entry(row)
                    .or_default()
                    .insert(column, overridden_cell(Some(&cell), value.as_ref()));
                return;
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
            write_csv_row(
                &mut output,
                out_cols,
                &pending_cells,
                override_rows.get(&row),
                cancel,
                out,
            )?;
            next_output_row = row.saturating_add(1);
        }
        while next_output_row < out_rows {
            write_csv_row(
                &mut output,
                out_cols,
                &[],
                override_rows.get(&next_output_row),
                cancel,
                out,
            )?;
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

    let mut dropped = CSV_DROPPED
        .iter()
        .map(|entry| (*entry).to_owned())
        .collect::<Vec<_>>();
    if dropped_size_overrides > 0 {
        dropped.push(format!(
            "size overrides are not representable in csv ({dropped_size_overrides})"
        ));
    }
    if !replaced_formulas.is_empty() {
        dropped.push(format!(
            "formulas replaced by edited values ({})",
            replaced_formulas.len()
        ));
    }
    Ok(ExportOutcome {
        bytes,
        applied,
        dropped,
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

/// Post-generation surgery for one worksheet: formula normalization plus
/// exact size fidelity. rust_xlsxwriter quantizes row heights to whole
/// pixels (0.75 pt steps) and has no default-column-width setter, so the
/// source's exact values are patched back into the generated XML.
#[derive(Clone, Debug, Default)]
struct SheetXmlPatch {
    formulas: Vec<FormulaPatch>,
    /// Exact heights keyed by the 1-based `r` attribute of `<row>`.
    row_heights: HashMap<u32, f64>,
    /// Exact widths keyed by the 1-based `min`/`max` range values of `<col>`.
    col_widths: HashMap<u32, f64>,
    default_row_height: Option<f64>,
    default_col_width: Option<f64>,
}

impl SheetXmlPatch {
    fn is_empty(&self) -> bool {
        self.formulas.is_empty()
            && self.row_heights.is_empty()
            && self.col_widths.is_empty()
            && self.default_row_height.is_none()
            && self.default_col_width.is_none()
    }
}

/// Shortest-representation float for XML attribute values.
fn xml_float(value: f64) -> String {
    format!("{value}")
}

fn normalize_generated_xml(
    source: NamedTempFile,
    out: &Path,
    sheets: &[SheetXmlPatch],
    cancel: &AtomicBool,
) -> Result<NamedTempFile, WriteError> {
    if sheets.iter().all(SheetXmlPatch::is_empty) {
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
                .filter(|patch| !patch.is_empty());
            if let Some(patches) = patches {
                normalize_worksheet_xml(&mut entry, &mut output, patches, out, cancel)?;
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

/// Rewrite `<sheetFormatPr>` with the source's exact defaults: the
/// generated `defaultRowHeight` is pixel-quantized, and `defaultColWidth`
/// is never generated at all.
fn patched_sheet_format_pr(start: &BytesStart<'_>, patch: &SheetXmlPatch) -> BytesStart<'static> {
    let mut element = BytesStart::new("sheetFormatPr");
    for attribute in start.attributes().with_checks(false).filter_map(Result::ok) {
        let key = attribute.key.as_ref();
        if key == b"defaultColWidth"
            || (key == b"defaultRowHeight" && patch.default_row_height.is_some())
        {
            continue;
        }
        element.push_attribute(attribute);
    }
    if let Some(height) = patch.default_row_height {
        element.push_attribute(("defaultRowHeight", xml_float(height).as_str()));
    }
    if let Some(width) = patch.default_col_width {
        element.push_attribute(("defaultColWidth", xml_float(width).as_str()));
    }
    element
}

/// Rewrite one generated `<col>` element with the source's exact
/// character-unit widths. rust_xlsxwriter quantizes widths to pixels, so
/// two source columns with distinct exact widths can collapse into one
/// generated range; the range is re-split into runs of equal exact width.
fn patched_col_segments(
    start: &BytesStart<'_>,
    widths: &HashMap<u32, f64>,
    out: &Path,
) -> Result<Vec<BytesStart<'static>>, WriteError> {
    let attribute_value = |key: &[u8]| {
        start
            .attributes()
            .with_checks(false)
            .filter_map(Result::ok)
            .find(|attribute| attribute.key.as_ref() == key)
            .and_then(|attribute| {
                std::str::from_utf8(attribute.value.as_ref())
                    .ok()
                    .and_then(|value| value.parse::<u32>().ok())
            })
    };
    let (Some(min), Some(max)) = (attribute_value(b"min"), attribute_value(b"max")) else {
        return Err(WriteError::new(
            "internal",
            format!(
                "generated col element without min/max range in {}",
                out.display()
            ),
        ));
    };
    if min > max || !(min..=max).any(|column| widths.contains_key(&column)) {
        return Ok(vec![start.to_owned().into_owned()]);
    }

    let mut segments = Vec::new();
    let mut cursor = min;
    while cursor <= max {
        let width = widths.get(&cursor).copied();
        let mut end = cursor;
        while end < max && widths.get(&(end + 1)).copied() == width {
            end += 1;
        }
        let mut element = BytesStart::new("col");
        element.push_attribute(("min", cursor.to_string().as_str()));
        element.push_attribute(("max", end.to_string().as_str()));
        if let Some(width) = width {
            element.push_attribute(("width", xml_float(width).as_str()));
        }
        for attribute in start.attributes().with_checks(false).filter_map(Result::ok) {
            match attribute.key.as_ref() {
                b"min" | b"max" => {}
                b"width" if width.is_some() => {}
                _ => element.push_attribute(attribute),
            }
        }
        segments.push(element);
        cursor = match end.checked_add(1) {
            Some(next) => next,
            None => break,
        };
    }
    Ok(segments)
}

/// Rewrite a `<row>`'s `ht` attribute with the source's exact height when
/// the row is patched (the generated value is pixel-quantized). Rows the
/// source did NOT declare a height for get `ht`/`customHeight` stripped:
/// with a custom default row height, rust_xlsxwriter stamps the default
/// onto every populated row, which would turn sparse declarations dense
/// on read-back.
fn patched_row_height(start: &BytesStart<'_>, patch: &SheetXmlPatch) -> BytesStart<'static> {
    let height = start
        .attributes()
        .with_checks(false)
        .filter_map(Result::ok)
        .find(|attribute| attribute.key.as_ref() == b"r")
        .and_then(|attribute| {
            std::str::from_utf8(attribute.value.as_ref())
                .ok()
                .and_then(|row| row.parse::<u32>().ok())
        })
        .and_then(|row| patch.row_heights.get(&row).copied());
    let mut element = BytesStart::new("row");
    for attribute in start.attributes().with_checks(false).filter_map(Result::ok) {
        match attribute.key.as_ref() {
            b"ht" => {}
            b"customHeight" if height.is_none() => {}
            _ => element.push_attribute(attribute),
        }
    }
    if let Some(height) = height {
        element.push_attribute(("ht", xml_float(height).as_str()));
    }
    element
}

fn normalize_worksheet_xml(
    input: &mut impl std::io::Read,
    output: &mut impl Write,
    patch: &SheetXmlPatch,
    out: &Path,
    cancel: &AtomicBool,
) -> Result<(), WriteError> {
    let patches = &patch.formulas;
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
            Event::Start(start) if start.name().as_ref() == b"sheetFormatPr" => {
                let start = patched_sheet_format_pr(&start, patch);
                writer
                    .write_event(Event::Start(start))
                    .map_err(|error| io_error("write normalized worksheet XML", out, error))?;
            }
            Event::Empty(start) if start.name().as_ref() == b"sheetFormatPr" => {
                let start = patched_sheet_format_pr(&start, patch);
                writer
                    .write_event(Event::Empty(start))
                    .map_err(|error| io_error("write normalized worksheet XML", out, error))?;
            }
            Event::Empty(start)
                if start.name().as_ref() == b"col" && !patch.col_widths.is_empty() =>
            {
                for element in patched_col_segments(&start, &patch.col_widths, out)? {
                    writer
                        .write_event(Event::Empty(element))
                        .map_err(|error| io_error("write normalized worksheet XML", out, error))?;
                }
            }
            Event::Start(start)
                if start.name().as_ref() == b"row"
                    && (!patch.row_heights.is_empty() || patch.default_row_height.is_some()) =>
            {
                let start = patched_row_height(&start, patch);
                writer
                    .write_event(Event::Start(start))
                    .map_err(|error| io_error("write normalized worksheet XML", out, error))?;
            }
            Event::Empty(start)
                if start.name().as_ref() == b"row"
                    && (!patch.row_heights.is_empty() || patch.default_row_height.is_some()) =>
            {
                let start = patched_row_height(&start, patch);
                writer
                    .write_event(Event::Empty(start))
                    .map_err(|error| io_error("write normalized worksheet XML", out, error))?;
            }
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

/// xlsx row heights live in 0..=409.5 points.
fn clamp_row_height(height: f64) -> f64 {
    if height.is_nan() {
        0.0
    } else {
        height.clamp(0.0, 409.5)
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
    overrides: Option<&BTreeMap<u32, WindowCell>>,
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
        let stored = cells.get(cell_index).and_then(|(cell_column, cell)| {
            (*cell_column == column).then(|| {
                cell_index += 1;
                cell
            })
        });
        let cell = overrides
            .and_then(|row_cells| row_cells.get(&column))
            .or(stored);
        if let Some(cell) = cell {
            write_csv_field(output, cell)
                .map_err(|error| io_error("write CSV output", out, error))?;
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
    use wax_core::{Cell, ColInfo, ColSizeOverride, Document, RowInfo, RowSizeOverride, Sheet};
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
            frozen_rows: 0,
            frozen_cols: 0,
            col_infos: Vec::new(),
            row_infos: Vec::new(),
            default_row_height: None,
            default_col_width: None,
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
    fn xlsx_round_trips_row_heights_and_size_defaults_exactly() {
        let mut model = sheet("Sizes", 3, 2, Vec::new(), &[]);
        model.col_infos = vec![ColInfo { c: 1, width: 22.5 }];
        // 30.6 is not representable in rust_xlsxwriter's whole-pixel
        // quantization (0.75 pt steps); exact survival proves the XML patch.
        model.row_infos = vec![
            RowInfo {
                r: 0,
                height: 27.75,
            },
            RowInfo { r: 2, height: 30.6 },
            RowInfo { r: 5, height: 45.0 },
        ];
        model.default_row_height = Some(14.4);
        model.default_col_width = Some(9.14);
        let store = store_with(vec![model], Vec::new());
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("sizes.xlsx");

        let outcome = write_xlsx(&store, &out, &AtomicBool::new(false)).expect("xlsx should write");
        assert_eq!(outcome.dropped, [] as [String; 0]);

        let actual = read_xlsx(&out);
        let sheet = &actual.sheets[0];
        assert_eq!(sheet.col_infos, vec![ColInfo { c: 1, width: 22.5 }]);
        assert_eq!(
            sheet.row_infos,
            vec![
                RowInfo {
                    r: 0,
                    height: 27.75,
                },
                RowInfo { r: 2, height: 30.6 },
                RowInfo { r: 5, height: 45.0 },
            ]
        );
        assert_eq!(sheet.default_row_height, Some(14.4));
        assert_eq!(sheet.default_col_width, Some(9.14));
    }

    #[test]
    fn xlsx_size_overrides_apply_last_wins_and_round_trip_exactly() {
        let mut model = sheet("Sizes", 3, 2, Vec::new(), &[]);
        model.col_infos = vec![ColInfo { c: 1, width: 22.5 }];
        model.row_infos = vec![RowInfo {
            r: 0,
            height: 27.75,
        }];
        let store = store_with(vec![model], Vec::new());
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("size-overrides.xlsx");

        let sizes = SizeOverrides {
            cols: vec![
                // Replaces the declared width, then loses to the later entry.
                ColSizeOverride {
                    sheet: 0,
                    c: 1,
                    width: 30.0,
                },
                ColSizeOverride {
                    sheet: 0,
                    c: 1,
                    width: 18.6,
                },
                // A column the source declared nothing for.
                ColSizeOverride {
                    sheet: 0,
                    c: 3,
                    width: 40.25,
                },
            ],
            rows: vec![RowSizeOverride {
                sheet: 0,
                r: 4,
                height: 33.3,
            }],
        };
        let outcome = write_xlsx_with_overrides(&store, &out, &[], &sizes, &AtomicBool::new(false))
            .expect("xlsx export should work");
        assert_eq!(outcome.dropped, [] as [String; 0]);

        let sheet = &read_xlsx(&out).sheets[0];
        assert_eq!(
            sheet.col_infos,
            vec![
                ColInfo { c: 1, width: 18.6 },
                ColInfo { c: 3, width: 40.25 },
            ]
        );
        assert_eq!(
            sheet.row_infos,
            vec![
                RowInfo {
                    r: 0,
                    height: 27.75,
                },
                RowInfo { r: 4, height: 33.3 },
            ]
        );
    }

    #[test]
    fn size_overrides_reject_unknown_sheets_and_grid_overflow_and_clamp_loudly() {
        let store = store_with(vec![sheet("Sheet1", 2, 2, Vec::new(), &[])], Vec::new());
        let temp = tempfile::tempdir().expect("temporary directory");
        let cancel = AtomicBool::new(false);

        let error = write_xlsx_with_overrides(
            &store,
            &temp.path().join("a.xlsx"),
            &[],
            &SizeOverrides {
                cols: vec![ColSizeOverride {
                    sheet: 3,
                    c: 0,
                    width: 9.0,
                }],
                rows: Vec::new(),
            },
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error.code, "bad_request");
        assert!(error.msg.contains("sheet index 3"), "{}", error.msg);

        let error = write_xlsx_with_overrides(
            &store,
            &temp.path().join("b.xlsx"),
            &[],
            &SizeOverrides {
                cols: Vec::new(),
                rows: vec![RowSizeOverride {
                    sheet: 0,
                    r: 1_048_576,
                    height: 15.0,
                }],
            },
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error.code, "bad_request");
        assert!(error.msg.contains("1048576"), "{}", error.msg);

        let out = temp.path().join("clamped.xlsx");
        let outcome = write_xlsx_with_overrides(
            &store,
            &out,
            &[],
            &SizeOverrides {
                cols: vec![ColSizeOverride {
                    sheet: 0,
                    c: 0,
                    width: 300.0,
                }],
                rows: vec![RowSizeOverride {
                    sheet: 0,
                    r: 0,
                    height: -3.0,
                }],
            },
            &cancel,
        )
        .expect("clamped size overrides should export");
        assert_eq!(
            outcome.dropped,
            [CLAMPED_COLUMN_WIDTHS_DROPPED, CLAMPED_ROW_HEIGHTS_DROPPED]
        );
        let sheet = &read_xlsx(&out).sheets[0];
        assert_eq!(sheet.col_infos, vec![ColInfo { c: 0, width: 255.0 }]);
        assert_eq!(sheet.row_infos, vec![RowInfo { r: 0, height: 0.0 }]);
    }

    #[test]
    fn csv_drops_size_overrides_loudly_and_scopes_to_the_exported_sheet() {
        let store = store_with(
            vec![
                sheet("First", 1, 1, Vec::new(), &[]),
                sheet("Second", 1, 1, Vec::new(), &[]),
            ],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("sizes.csv");
        let sizes = SizeOverrides {
            cols: vec![
                ColSizeOverride {
                    sheet: 0,
                    c: 0,
                    width: 9.0,
                },
                // Duplicate collapses last-wins before counting.
                ColSizeOverride {
                    sheet: 0,
                    c: 0,
                    width: 12.0,
                },
                // Another sheet's entry is ignored by the csv scope.
                ColSizeOverride {
                    sheet: 1,
                    c: 0,
                    width: 9.0,
                },
            ],
            rows: vec![RowSizeOverride {
                sheet: 0,
                r: 0,
                height: 20.0,
            }],
        };
        let outcome =
            write_csv_with_overrides(&store, 0, &out, &[], &sizes, &AtomicBool::new(false))
                .expect("csv export should work");
        assert!(
            outcome
                .dropped
                .contains(&"size overrides are not representable in csv (2)".to_owned()),
            "{:?}",
            outcome.dropped
        );

        // Overrides aimed only at other sheets stay silent.
        let outcome = write_csv_with_overrides(
            &store,
            1,
            &out,
            &[],
            &SizeOverrides {
                cols: vec![ColSizeOverride {
                    sheet: 0,
                    c: 0,
                    width: 9.0,
                }],
                rows: Vec::new(),
            },
            &AtomicBool::new(false),
        )
        .expect("csv export should work");
        assert!(
            !outcome
                .dropped
                .iter()
                .any(|entry| entry.contains("size overrides")),
            "{:?}",
            outcome.dropped
        );
    }

    #[test]
    fn xlsx_clamps_out_of_range_row_heights_loudly() {
        let mut model = sheet("Heights", 2, 1, Vec::new(), &[]);
        model.row_infos = vec![
            RowInfo { r: 0, height: -5.0 },
            RowInfo {
                r: 1,
                height: 500.0,
            },
        ];
        let store = store_with(vec![model], Vec::new());
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("clamped-heights.xlsx");

        let outcome = write_xlsx(&store, &out, &AtomicBool::new(false))
            .expect("out-of-range heights should be clamped");
        assert_eq!(outcome.dropped, [CLAMPED_ROW_HEIGHTS_DROPPED]);
        assert_eq!(clamp_row_height(-5.0), 0.0);
        assert_eq!(clamp_row_height(500.0), 409.5);
        let heights = read_xlsx(&out).sheets[0]
            .row_infos
            .iter()
            .map(|info| (info.r, info.height))
            .collect::<Vec<_>>();
        assert!(heights.contains(&(1, 409.5)), "{heights:?}");
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
                "row heights",
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

    // ---- v0.2 export-with-overrides (docs/v0.2-overrides-contract.md) ----

    fn ov(sheet: u32, r: u32, c: u32, v: Option<CellValue>) -> CellOverride {
        CellOverride { sheet, r, c, v }
    }

    fn csv_with_overrides(
        store: &WorkbookStore,
        overrides: &[CellOverride],
    ) -> (String, ExportOutcome) {
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("overrides.csv");
        let outcome = write_csv_with_overrides(
            store,
            0,
            &out,
            overrides,
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("csv export should work");
        (
            std::fs::read_to_string(&out).expect("csv should be readable"),
            outcome,
        )
    }

    /// Amendment A1, the mandatory regression: CSV is display-else-raw, so a
    /// stale `d` would silently export the OLD value. The override must
    /// recompute `d` through the retained format code.
    #[test]
    fn a1_csv_exports_the_recomputed_display_never_the_stale_one() {
        let store = store_with(
            vec![sheet(
                "S",
                1,
                1,
                vec![cell(
                    0,
                    0,
                    CellType::N,
                    Some(CellValue::Number(1.0)),
                    Some("1.00"),
                    None,
                    Some("0.00"),
                    None,
                )],
                &[],
            )],
            Vec::new(),
        );

        let (csv, outcome) =
            csv_with_overrides(&store, &[ov(0, 0, 0, Some(CellValue::Number(2.0)))]);
        assert_eq!(csv, "2.00\r\n");
        assert_eq!(outcome.applied, 1);

        // The same override through the xlsx path replaces the value.
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("overrides.xlsx");
        let outcome = write_xlsx_with_overrides(
            &store,
            &out,
            &[ov(0, 0, 0, Some(CellValue::Number(2.0)))],
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("xlsx export should work");
        assert_eq!(outcome.applied, 1);
        let actual = read_xlsx(&out);
        let cells = cells_by_position(&actual, 0);
        assert_eq!(cells[&(0, 0)].v, Some(CellValue::Number(2.0)));
    }

    /// Amendment A1: with no retained format code `d` becomes null and CSV
    /// falls back to the raw value, ignoring any stale display string.
    #[test]
    fn a1_display_falls_back_to_raw_without_a_retained_format_code() {
        let store = store_with(
            vec![sheet(
                "S",
                1,
                2,
                vec![
                    cell(
                        0,
                        0,
                        CellType::S,
                        Some(CellValue::Text("old".to_owned())),
                        Some("OLD DISPLAY"),
                        None,
                        None,
                        None,
                    ),
                    cell(
                        0,
                        1,
                        CellType::S,
                        Some(CellValue::Text("keep".to_owned())),
                        Some("KEEP"),
                        None,
                        None,
                        None,
                    ),
                ],
                &[],
            )],
            Vec::new(),
        );
        let (csv, outcome) = csv_with_overrides(
            &store,
            &[
                ov(0, 0, 0, Some(CellValue::Number(2.5))),
                ov(0, 1, 0, Some(CellValue::Bool(true))),
                ov(0, 1, 1, Some(CellValue::Text("=1+2".to_owned()))),
            ],
        );
        assert_eq!(csv, "2.5,KEEP\r\nTRUE,=1+2\r\n");
        assert_eq!(outcome.applied, 3);
    }

    /// Amendment A2: overriding an existing cell keeps its style id and
    /// format code; a cell created beyond the original extent has neither
    /// and lands on the XF-0 base style.
    #[test]
    fn a2_overrides_keep_style_and_format_and_new_cells_land_on_xf0() {
        let store = store_with(
            vec![sheet(
                "S",
                1,
                1,
                vec![cell(
                    0,
                    0,
                    CellType::N,
                    Some(CellValue::Number(1.0)),
                    None,
                    None,
                    Some("0.00"),
                    Some(0),
                )],
                &[],
            )],
            vec![CellStyle {
                bold: true,
                ..CellStyle::default()
            }],
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("styled.xlsx");
        write_xlsx_with_overrides(
            &store,
            &out,
            &[
                ov(0, 0, 0, Some(CellValue::Number(3.0))),
                ov(0, 0, 2, Some(CellValue::Number(7.0))),
            ],
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("xlsx export should work");

        let styles = zip_text(&out, "xl/styles.xml");
        assert!(styles.contains("<b/>"), "{styles}");
        let actual = read_xlsx(&out);
        let cells = cells_by_position(&actual, 0);
        let overridden = cells[&(0, 0)];
        assert_eq!(overridden.v, Some(CellValue::Number(3.0)));
        assert_eq!(overridden.fmt.as_deref(), Some("0.00"));
        assert_eq!(overridden.d.as_deref(), Some("3.00"));
        let style = overridden
            .s
            .and_then(|id| actual.styles.get(id as usize))
            .expect("overridden cell should keep a style");
        assert!(style.bold);
        let synthesized = cells[&(0, 2)];
        assert_eq!(synthesized.v, Some(CellValue::Number(7.0)));
        assert_eq!(synthesized.fmt, None);
        let synthesized_bold = synthesized
            .s
            .and_then(|id| actual.styles.get(id as usize))
            .is_some_and(|style| style.bold);
        assert!(
            !synthesized_bold,
            "synthesized cell must not inherit styling"
        );
    }

    /// Amendment A3: a numeric override into a date-formatted cell is stored
    /// as a number; the retained format code makes wax's own reader type it
    /// as a date on read-back, and the recomputed display shows the date.
    #[test]
    fn a3_numeric_override_is_never_coerced_but_reads_back_as_a_date() {
        let store = store_with(
            vec![sheet(
                "S",
                1,
                1,
                vec![cell(
                    0,
                    0,
                    CellType::D,
                    Some(CellValue::Text("2020-01-01".to_owned())),
                    Some("2020-01-01"),
                    None,
                    Some("yyyy-mm-dd"),
                    None,
                )],
                &[],
            )],
            Vec::new(),
        );

        // Serial 45000 in the 1900 system is 2023-03-15.
        let (csv, _) = csv_with_overrides(&store, &[ov(0, 0, 0, Some(CellValue::Number(45000.0)))]);
        assert_eq!(csv, "2023-03-15\r\n");

        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("date.xlsx");
        write_xlsx_with_overrides(
            &store,
            &out,
            &[ov(0, 0, 0, Some(CellValue::Number(45000.0)))],
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("xlsx export should work");
        let raw_sheet = zip_text(&out, "xl/worksheets/sheet1.xml");
        assert!(
            raw_sheet.contains("<v>45000</v>"),
            "the stored value must stay the raw number: {raw_sheet}"
        );
        let actual = read_xlsx(&out);
        let cells = cells_by_position(&actual, 0);
        assert_eq!(cells[&(0, 0)].t, CellType::D);
        assert_eq!(
            cells[&(0, 0)].v,
            Some(CellValue::Text("2023-03-15".to_owned()))
        );
    }

    /// Amendment A4: duplicates are last-wins in array order and `applied`
    /// counts post-collapse distinct cells.
    #[test]
    fn a4_duplicate_overrides_are_last_wins_and_applied_counts_post_collapse() {
        let store = store_with(vec![sheet("S", 1, 2, Vec::new(), &[])], Vec::new());
        let overrides = [
            ov(0, 0, 0, Some(CellValue::Number(1.0))),
            ov(0, 0, 1, Some(CellValue::Number(3.0))),
            ov(0, 0, 0, Some(CellValue::Number(2.0))),
        ];
        let (csv, outcome) = csv_with_overrides(&store, &overrides);
        assert_eq!(csv, "2,3\r\n");
        assert_eq!(outcome.applied, 2);

        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("dupes.xlsx");
        let outcome = write_xlsx_with_overrides(
            &store,
            &out,
            &overrides,
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("xlsx export should work");
        assert_eq!(outcome.applied, 2);
        let actual = read_xlsx(&out);
        let cells = cells_by_position(&actual, 0);
        assert_eq!(cells[&(0, 0)].v, Some(CellValue::Number(2.0)));
    }

    /// Amendment A5: bad requests (cap, unknown sheet) are `bad_request`;
    /// extent growth breaching the cell caps is `bomb` or `too_large`.
    #[test]
    fn a5_error_taxonomy_splits_bad_request_from_extent_rails() {
        let store = store_with(vec![sheet("S", 1, 1, Vec::new(), &[])], Vec::new());
        let temp = tempfile::tempdir().expect("temporary directory");
        let cancel = AtomicBool::new(false);

        let unknown_sheet = [ov(5, 0, 0, Some(CellValue::Number(1.0)))];
        for error in [
            write_xlsx_with_overrides(
                &store,
                &temp.path().join("a.xlsx"),
                &unknown_sheet,
                &SizeOverrides::default(),
                &cancel,
            )
            .unwrap_err(),
            write_csv_with_overrides(
                &store,
                0,
                &temp.path().join("a.csv"),
                &unknown_sheet,
                &SizeOverrides::default(),
                &cancel,
            )
            .unwrap_err(),
        ] {
            assert_eq!(error.code, "bad_request");
            assert!(error.msg.contains("sheet index 5"), "{}", error.msg);
        }

        let over_cap = (0..u32::try_from(EXPORT_OVERRIDES_CAP).unwrap() + 1)
            .map(|index| ov(0, 0, index % 3, Some(CellValue::Number(1.0))))
            .collect::<Vec<_>>();
        let error = write_csv_with_overrides(
            &store,
            0,
            &temp.path().join("b.csv"),
            &over_cap,
            &SizeOverrides::default(),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error.code, "bad_request");
        assert!(error.msg.contains("100000-entry cap"), "{}", error.msg);

        let bomb = [ov(0, 9_000_000, 0, Some(CellValue::Number(1.0)))];
        let error = write_csv_with_overrides(
            &store,
            0,
            &temp.path().join("c.csv"),
            &bomb,
            &SizeOverrides::default(),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error.code, "bomb");
        assert!(error.msg.contains("8000000 cell limit"), "{}", error.msg);

        let error = write_xlsx_with_overrides(
            &store,
            &temp.path().join("d.xlsx"),
            &[ov(0, 1_048_576, 0, Some(CellValue::Number(1.0)))],
            &SizeOverrides::default(),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error.code, "too_large");
        assert!(error.msg.contains("1048576"), "{}", error.msg);

        let error = write_xlsx_with_overrides(
            &store,
            &temp.path().join("e.xlsx"),
            &[ov(0, 0, 16_384, Some(CellValue::Number(1.0)))],
            &SizeOverrides::default(),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error.code, "too_large");
        assert!(error.msg.contains("16384"), "{}", error.msg);

        let non_finite = [ov(0, 0, 0, Some(CellValue::Number(f64::NAN)))];
        for error in [
            write_xlsx_with_overrides(
                &store,
                &temp.path().join("f.xlsx"),
                &non_finite,
                &SizeOverrides::default(),
                &cancel,
            )
            .unwrap_err(),
            write_csv_with_overrides(
                &store,
                0,
                &temp.path().join("f.csv"),
                &non_finite,
                &SizeOverrides::default(),
                &cancel,
            )
            .unwrap_err(),
        ] {
            assert_eq!(error.code, "bad_request");
            assert!(error.msg.contains("finite"), "{}", error.msg);
        }
    }

    /// A large sheet whose extent already exceeds the cap still exports when
    /// overrides stay inside it — the bomb rail fires only on growth.
    #[test]
    fn a5_extent_bomb_fires_only_when_overrides_grow_the_extent() {
        // 9,000,000 declared cells, sparse: one stored value.
        let store = store_with(
            vec![sheet(
                "Big",
                9_000_000,
                1,
                vec![cell(
                    0,
                    0,
                    CellType::N,
                    Some(CellValue::Number(1.0)),
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
        let cancel = AtomicBool::new(false);
        // In-extent override on an over-cap sheet: allowed (no growth) — but
        // don't actually write 9M CSV rows here; assert collapse passes.
        let collapsed = collapse_overrides(
            &store,
            &[ov(0, 100, 0, Some(CellValue::Number(2.0)))],
            Some(0),
        )
        .expect("in-extent override on a large sheet should collapse");
        assert_eq!(collapsed[&0].cells.len(), 1);
        // Growth on the over-cap sheet: refused.
        let error = write_csv_with_overrides(
            &store,
            0,
            &temp.path().join("grown.csv"),
            &[ov(0, 9_000_000, 0, Some(CellValue::Number(2.0)))],
            &SizeOverrides::default(),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error.code, "bomb");
    }

    /// Amendment A6: CSV exports one sheet; overrides for other (valid)
    /// sheets are ignored, and `applied` counts only the exported sheet.
    #[test]
    fn a6_csv_ignores_other_sheet_overrides_and_counts_only_the_exported_sheet() {
        let mut second = sheet("Two", 1, 1, Vec::new(), &[]);
        second.index = 1;
        let store = store_with(
            vec![sheet("One", 1, 1, Vec::new(), &[]), second],
            Vec::new(),
        );
        let overrides = [
            ov(0, 0, 0, Some(CellValue::Text("first".to_owned()))),
            ov(1, 0, 0, Some(CellValue::Text("second".to_owned()))),
        ];
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("sheet0.csv");
        let outcome = write_csv_with_overrides(
            &store,
            0,
            &out,
            &overrides,
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("csv export should work");
        assert_eq!(outcome.applied, 1);
        assert_eq!(
            std::fs::read_to_string(&out).expect("csv should be readable"),
            "first\r\n"
        );
        let out = temp.path().join("sheet1.csv");
        let outcome = write_csv_with_overrides(
            &store,
            1,
            &out,
            &overrides,
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("csv export should work");
        assert_eq!(outcome.applied, 1);
        assert_eq!(
            std::fs::read_to_string(&out).expect("csv should be readable"),
            "second\r\n"
        );
        // xlsx applies the whole edit set.
        let out = temp.path().join("both.xlsx");
        let outcome = write_xlsx_with_overrides(
            &store,
            &out,
            &overrides,
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("xlsx export should work");
        assert_eq!(outcome.applied, 2);
        let actual = read_xlsx(&out);
        assert_eq!(
            cells_by_position(&actual, 0)[&(0, 0)].v,
            Some(CellValue::Text("first".to_owned()))
        );
        assert_eq!(
            cells_by_position(&actual, 1)[&(0, 0)].v,
            Some(CellValue::Text("second".to_owned()))
        );
    }

    /// Strings beginning with `=` are stored as text, never reinterpreted as
    /// formulas (contract: apiary phase-2 policy).
    #[test]
    fn override_strings_starting_with_equals_stay_text() {
        let store = store_with(vec![sheet("S", 1, 1, Vec::new(), &[])], Vec::new());
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("equals.xlsx");
        let outcome = write_xlsx_with_overrides(
            &store,
            &out,
            &[ov(0, 0, 0, Some(CellValue::Text("=SUM(A1:A2)".to_owned())))],
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("xlsx export should work");
        assert!(
            !outcome
                .dropped
                .iter()
                .any(|entry| entry.contains("formula")),
            "{:?}",
            outcome.dropped
        );
        let actual = read_xlsx(&out);
        let cells = cells_by_position(&actual, 0);
        assert_eq!(cells[&(0, 0)].f, None);
        assert_eq!(
            cells[&(0, 0)].v,
            Some(CellValue::Text("=SUM(A1:A2)".to_owned()))
        );
    }

    /// Overriding a formula cell drops the formula in favour of the literal
    /// value, recorded loudly with a count.
    #[test]
    fn overriding_formula_cells_drops_the_formulas_loudly_with_a_count() {
        let store = store_with(
            vec![sheet(
                "S",
                1,
                3,
                vec![
                    cell(
                        0,
                        0,
                        CellType::N,
                        Some(CellValue::Number(3.0)),
                        None,
                        Some("1+2"),
                        None,
                        None,
                    ),
                    cell(
                        0,
                        1,
                        CellType::N,
                        Some(CellValue::Number(4.0)),
                        None,
                        Some("2+2"),
                        None,
                        None,
                    ),
                    cell(
                        0,
                        2,
                        CellType::N,
                        Some(CellValue::Number(5.0)),
                        None,
                        Some("2+3"),
                        None,
                        None,
                    ),
                ],
                &[],
            )],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("formulas.xlsx");
        let overrides = [
            ov(0, 0, 0, Some(CellValue::Number(30.0))),
            ov(0, 0, 1, Some(CellValue::Number(40.0))),
        ];
        let outcome = write_xlsx_with_overrides(
            &store,
            &out,
            &overrides,
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("xlsx export should work");
        assert!(
            outcome
                .dropped
                .contains(&"formulas replaced by edited values (2)".to_owned()),
            "{:?}",
            outcome.dropped
        );
        let actual = read_xlsx(&out);
        let cells = cells_by_position(&actual, 0);
        assert_eq!(cells[&(0, 0)].f, None);
        assert_eq!(cells[&(0, 0)].v, Some(CellValue::Number(30.0)));
        assert_eq!(cells[&(0, 1)].f, None);
        assert_eq!(cells[&(0, 2)].f.as_deref(), Some("2+3"));
        assert_eq!(cells[&(0, 2)].v, Some(CellValue::Number(5.0)));

        let (_, outcome) = csv_with_overrides(&store, &overrides[..1]);
        assert!(
            outcome
                .dropped
                .contains(&"formulas replaced by edited values (1)".to_owned()),
            "{:?}",
            outcome.dropped
        );
    }

    /// `v: null` clears the cell; clearing keeps a formatted cell's styling
    /// (blank write) and clearing a cell that holds nothing is a counted
    /// no-op.
    #[test]
    fn null_override_clears_the_cell() {
        let store = store_with(
            vec![sheet(
                "S",
                1,
                2,
                vec![
                    cell(
                        0,
                        0,
                        CellType::S,
                        Some(CellValue::Text("gone".to_owned())),
                        Some("gone"),
                        None,
                        Some("@"),
                        None,
                    ),
                    cell(
                        0,
                        1,
                        CellType::N,
                        Some(CellValue::Number(5.0)),
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
        let (csv, outcome) = csv_with_overrides(&store, &[ov(0, 0, 0, None)]);
        assert_eq!(csv, ",5\r\n");
        assert_eq!(outcome.applied, 1);

        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("cleared.xlsx");
        let outcome = write_xlsx_with_overrides(
            &store,
            &out,
            // The second entry clears a cell that holds nothing:
            // a no-op that is still accepted and counted.
            &[ov(0, 0, 0, None), ov(0, 1, 0, None)],
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("xlsx export should work");
        assert_eq!(outcome.applied, 2);
        let actual = read_xlsx(&out);
        let cells = cells_by_position(&actual, 0);
        assert!(
            cells.get(&(0, 0)).is_none_or(|cell| cell.v.is_none()),
            "cleared cell must hold no value"
        );
        assert_eq!(cells[&(0, 1)].v, Some(CellValue::Number(5.0)));
        // The cleared value is gone from the sheet XML entirely.
        let raw_sheet = zip_text(&out, "xl/worksheets/sheet1.xml");
        assert!(!raw_sheet.contains("gone"), "{raw_sheet}");
    }

    /// Overrides may extend the used extent: out-of-extent cells are
    /// synthesized and dims are recomputed as max(store, override max + 1).
    #[test]
    fn overrides_extend_the_extent_and_recompute_dims() {
        let store = store_with(
            vec![sheet(
                "S",
                2,
                2,
                vec![cell(
                    0,
                    0,
                    CellType::N,
                    Some(CellValue::Number(1.0)),
                    None,
                    None,
                    None,
                    None,
                )],
                &[],
            )],
            Vec::new(),
        );
        let overrides = [ov(0, 4, 3, Some(CellValue::Number(9.0)))];
        let (csv, outcome) = csv_with_overrides(&store, &overrides);
        assert_eq!(csv, "1,,,\r\n,,,\r\n,,,\r\n,,,\r\n,,,9\r\n");
        assert_eq!(outcome.applied, 1);

        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("extended.xlsx");
        write_xlsx_with_overrides(
            &store,
            &out,
            &overrides,
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("xlsx export should work");
        let actual = read_xlsx(&out);
        assert_eq!(actual.sheets[0].rows, 5);
        assert_eq!(actual.sheets[0].cols, 4);
        assert_eq!(
            cells_by_position(&actual, 0)[&(4, 3)].v,
            Some(CellValue::Number(9.0))
        );
    }

    /// The store is shared across handles: export must never mutate it, so a
    /// second export of the same store is unaffected by earlier overrides.
    #[test]
    fn export_with_overrides_is_side_effect_free() {
        let store = store_with(
            vec![sheet(
                "S",
                1,
                1,
                vec![cell(
                    0,
                    0,
                    CellType::N,
                    Some(CellValue::Number(1.0)),
                    Some("1.00"),
                    None,
                    Some("0.00"),
                    None,
                )],
                &[],
            )],
            Vec::new(),
        );
        let (csv, _) = csv_with_overrides(&store, &[ov(0, 0, 0, Some(CellValue::Number(2.0)))]);
        assert_eq!(csv, "2.00\r\n");
        let (csv, outcome) = csv_with_overrides(&store, &[]);
        assert_eq!(csv, "1.00\r\n");
        assert_eq!(outcome.applied, 0);
    }

    #[test]
    fn evaluated_formula_cache_round_trips_without_dropping_the_formula() {
        let store = store_with(
            vec![sheet(
                "S",
                1,
                1,
                vec![cell(
                    0,
                    0,
                    CellType::N,
                    Some(CellValue::Number(5.0)),
                    Some("5"),
                    Some("2+11"),
                    Some("0.00"),
                    None,
                )],
                &[],
            )],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("evaluated.xlsx");
        write_xlsx_with_evaluated_overrides(
            &store,
            &out,
            &[],
            &SizeOverrides::default(),
            &[EvaluatedCell {
                sheet: 0,
                r: 0,
                c: 0,
                t: CellType::N,
                v: Some(CellValue::Number(13.0)),
                d: Some("13.00".to_owned()),
                e: true,
            }],
            &AtomicBool::new(false),
        )
        .expect("evaluated export should succeed");

        let actual = read_xlsx(&out);
        let round_trip = &cells_by_position(&actual, 0)[&(0, 0)];
        assert_eq!(round_trip.f.as_deref(), Some("2+11"));
        assert_eq!(round_trip.v, Some(CellValue::Number(13.0)));
        assert_eq!(round_trip.fmt.as_deref(), Some("0.00"));
        assert_eq!(
            store.cell(0, 0, 0).and_then(|cell| cell.v),
            Some(CellValue::Number(5.0)),
            "export must not mutate the retained store"
        );
    }

    /// Contract note 1: the W5D oversized-string policy applies to overrides
    /// — truncate on a character boundary, drop loudly, still succeed.
    #[test]
    fn oversized_string_override_truncates_loudly_and_the_export_succeeds() {
        let store = store_with(vec![sheet("S", 1, 1, Vec::new(), &[])], Vec::new());
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("long.xlsx");
        let oversized = "x".repeat(XLSX_MAX_STRING_CHARS + 1);
        let outcome = write_xlsx_with_overrides(
            &store,
            &out,
            &[ov(0, 0, 0, Some(CellValue::Text(oversized)))],
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("xlsx export should succeed despite the oversized string");
        assert!(
            outcome
                .dropped
                .iter()
                .any(|entry| entry.contains("truncated from 32768 to 32767")),
            "{:?}",
            outcome.dropped
        );
        let actual = read_xlsx(&out);
        let cells = cells_by_position(&actual, 0);
        let Some(CellValue::Text(text)) = &cells[&(0, 0)].v else {
            panic!("expected a text cell");
        };
        assert_eq!(text.chars().count(), XLSX_MAX_STRING_CHARS);
    }

    /// Overrides work against cells inside merged ranges (anchor or not).
    #[test]
    fn overrides_apply_inside_merged_ranges() {
        let store = store_with(
            vec![sheet(
                "S",
                2,
                2,
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
                &["A1:B2"],
            )],
            Vec::new(),
        );
        let temp = tempfile::tempdir().expect("temporary directory");
        let out = temp.path().join("merged.xlsx");
        write_xlsx_with_overrides(
            &store,
            &out,
            &[
                ov(0, 0, 0, Some(CellValue::Text("edited".to_owned()))),
                ov(0, 1, 1, Some(CellValue::Text("corner".to_owned()))),
            ],
            &SizeOverrides::default(),
            &AtomicBool::new(false),
        )
        .expect("xlsx export should work");
        let actual = read_xlsx(&out);
        let cells = cells_by_position(&actual, 0);
        assert_eq!(cells[&(0, 0)].v, Some(CellValue::Text("edited".to_owned())));
        assert_eq!(cells[&(1, 1)].v, Some(CellValue::Text("corner".to_owned())));
    }
}

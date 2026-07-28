use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::{Duration, Instant};

use calamine::{
    expand_shared_formula, open_workbook, Data, Dimensions, Ods, Range, Reader as CalamineWorkbook,
    SheetType, Xls, Xlsb, Xlsx, XlsxFormulaMetadata,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader as XmlReader;
use wax_core::{Cell, CellType, CellValue, Document, DumpError, Sheet};
use wax_fmt::{render, FmtValue};
use wax_proto::ErrorCode;
use zip::ZipArchive;

use crate::{Reader, ReaderOptions};

/// Workbook reader backed by calamine, with wax's normalization layered on top.
#[derive(Clone, Copy, Debug, Default)]
pub struct CalamineReader;

impl Reader for CalamineReader {
    fn read(&self, path: &Path, options: ReaderOptions) -> Document {
        let file = path.to_string_lossy().into_owned();
        let result = catch_unwind(AssertUnwindSafe(|| read_workbook(path, options)));
        match result {
            Ok(Ok((sheets, warnings))) => {
                Document::success(env!("CARGO_PKG_VERSION"), file, sheets, warnings)
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

fn read_workbook(
    path: &Path,
    options: ReaderOptions,
) -> Result<(Vec<Sheet>, Vec<String>), ReadFailure> {
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
        WorkbookKind::Xlsb => read_xlsb(path, options.max_cells, started, timeout),
        WorkbookKind::Xls => read_xls(path, options.max_cells, started, timeout),
        WorkbookKind::Ods => read_ods(path, options.max_cells, started, timeout),
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
) -> Result<(Vec<Sheet>, Vec<String>), ReadFailure> {
    let mut workbook: Xlsx<BufReader<File>> =
        open_workbook(path).map_err(|error| container_error(WorkbookKind::Xlsx, error))?;
    let epoch_1904 = workbook.has_1904_epoch();
    let metadata = workbook.sheets_metadata().to_vec();
    let mut warnings = Vec::new();
    let supplement = match OoxmlSupplement::read(path) {
        Ok(supplement) => supplement,
        Err(message) => {
            warnings.push(format!(
                "number-format metadata could not be read; fmt values may be null: {message}"
            ));
            OoxmlSupplement::default()
        }
    };

    let mut emitted = 0_usize;
    let mut sheets = Vec::with_capacity(metadata.len());
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
                if let Some(cell) = normalize_cell(
                    record.pos,
                    &Data::from(record.value),
                    formula,
                    cell_metadata.and_then(|metadata| metadata.format.as_deref()),
                    cell_metadata.and_then(|metadata| metadata.declared_type),
                    epoch_1904,
                ) {
                    candidates.push(cell);
                }
            }
        }

        let merges = workbook
            .merge_cells_by_sheet_name(&metadata.name)
            .map_err(|error| container_error(WorkbookKind::Xlsx, error))?;
        let (rows, cols) = extent_from_dimensions(dimensions, &candidates);
        sheets.push(finish_sheet(
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
        ));
    }
    Ok((sheets, warnings))
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
    let mut workbook: Xlsb<BufReader<File>> =
        open_workbook(path).map_err(|error| container_error(WorkbookKind::Xlsb, error))?;
    let epoch_1904 = workbook.has_1904_epoch();
    let metadata = workbook.sheets_metadata().to_vec();
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
                records.entry(cell.get_position()).or_default().value =
                    Data::from(cell.get_value().clone());
            }
        }
        {
            let mut formulas = workbook
                .worksheet_cells_reader(&metadata.name)
                .map_err(|error| container_error(WorkbookKind::Xlsb, error))?;
            let mut seen = 0_usize;
            while let Some(cell) = formulas
                .next_formula()
                .map_err(|error| container_error(WorkbookKind::Xlsb, error))?
            {
                seen += 1;
                if seen.is_multiple_of(4096) {
                    ensure_time_remaining(started, timeout)?;
                }
                records.entry(cell.get_position()).or_default().formula =
                    normalize_formula(cell.get_value().clone());
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

    Ok((
        sheets,
        vec!["xlsb number-format codes and merged regions are best-effort".to_owned()],
    ))
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
        let (records, rows, cols) = range_records(&values, &formulas);
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

    Ok((
        sheets,
        vec!["legacy xls number-format codes are best-effort".to_owned()],
    ))
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
        let (records, rows, cols) = range_records(&values, &formulas);
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
}

fn range_records(
    values: &Range<Data>,
    formulas: &Range<String>,
) -> (BTreeMap<(u32, u32), Candidate>, u32, u32) {
    let mut records = BTreeMap::<(u32, u32), Candidate>::new();
    let value_start = values.start().unwrap_or((0, 0));
    for (row, col, value) in values.used_cells() {
        let position = (
            value_start.0.saturating_add(row as u32),
            value_start.1.saturating_add(col as u32),
        );
        records.entry(position).or_default().value = value.clone();
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
                None,
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
    let is_empty = matches!(data, Data::Empty)
        || matches!(data, Data::String(value) if value.is_empty())
        || matches!(data, Data::Float(value) if !value.is_finite());
    if formula.is_none() && is_empty {
        return None;
    }

    let code = format.unwrap_or("General");
    let (cell_type, value, display) = match data {
        Data::Int(value) => {
            let raw = *value as f64;
            (
                CellType::N,
                Some(CellValue::Number(raw)),
                render(code, FmtValue::Number(raw), epoch_1904),
            )
        }
        Data::Float(value) if value.is_finite() => (
            CellType::N,
            Some(CellValue::Number(*value)),
            render(code, FmtValue::Number(*value), epoch_1904),
        ),
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
            let serial = value.as_f64();
            (
                CellType::D,
                Some(CellValue::Text(excel_datetime_iso(*value))),
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
        r: position.0,
        c: position.1,
        t: cell_type,
        v: value,
        d: display,
        f: formula,
        fmt: explicit_format(format),
    })
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

fn explicit_format(format: Option<&str>) -> Option<String> {
    format
        .filter(|format| !format.eq_ignore_ascii_case("general"))
        .map(str::to_owned)
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
    ) {
        ErrorCode::BadZip
    } else {
        ErrorCode::Internal
    };
    ReadFailure::new(code, message)
}

#[derive(Clone, Copy, Debug)]
struct CellMetadata {
    style_index: Option<usize>,
    declared_type: Option<CellType>,
}

impl CellMetadata {
    fn with_formats(self, formats: &[Option<String>]) -> ResolvedCellMetadata {
        ResolvedCellMetadata {
            format: self
                .style_index
                .and_then(|index| formats.get(index))
                .cloned()
                .flatten(),
            declared_type: self.declared_type,
        }
    }
}

#[derive(Clone, Debug)]
struct ResolvedCellMetadata {
    format: Option<String>,
    declared_type: Option<CellType>,
}

#[derive(Default)]
struct OoxmlSupplement {
    sheets: HashMap<String, HashMap<(u32, u32), ResolvedCellMetadata>>,
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
        let formats = match read_part_optional(&mut archive, &lookup, &styles_path)? {
            Some(styles) => parse_styles(&styles)?,
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
            let cells = parse_sheet_metadata(&xml)?
                .into_iter()
                .map(|(position, metadata)| (position, metadata.with_formats(&formats)))
                .collect();
            sheets.insert(workbook_sheet.name, cells);
        }
        Ok(Self { sheets })
    }

    fn cell(&self, sheet: &str, position: (u32, u32)) -> Option<&ResolvedCellMetadata> {
        self.sheets.get(sheet)?.get(&position)
    }
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
        lookup.insert(entry.name().to_ascii_lowercase(), entry.name().to_owned());
    }
    Ok(lookup)
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
    let normalized = path.trim_start_matches('/').to_ascii_lowercase();
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

fn parse_styles(xml: &[u8]) -> Result<Vec<Option<String>>, String> {
    let mut reader = XmlReader::from_reader(xml);
    let mut buffer = Vec::new();
    let mut custom = HashMap::<u32, String>::new();
    let mut formats = Vec::new();
    let mut in_num_formats = false;
    let mut in_cell_xfs = false;

    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
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
            Ok(Event::Start(element) | Event::Empty(element))
                if in_num_formats && element.local_name().as_ref() == b"numFmt" =>
            {
                if let (Some(id), Some(code)) = (
                    xml_attribute(&reader, &element, b"numFmtId")?
                        .and_then(|id| id.parse::<u32>().ok()),
                    xml_attribute(&reader, &element, b"formatCode")?,
                ) {
                    custom.insert(id, code);
                }
            }
            Ok(Event::Start(element) | Event::Empty(element))
                if in_cell_xfs && element.local_name().as_ref() == b"xf" =>
            {
                let id = xml_attribute(&reader, &element, b"numFmtId")?
                    .and_then(|id| id.parse::<u32>().ok())
                    .unwrap_or(0);
                let format = custom
                    .get(&id)
                    .cloned()
                    .or_else(|| builtin_format(id).map(str::to_owned));
                formats.push(explicit_format(format.as_deref()));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("invalid styles XML: {error}")),
        }
    }
    Ok(formats)
}

fn parse_sheet_metadata(xml: &[u8]) -> Result<HashMap<(u32, u32), CellMetadata>, String> {
    let mut reader = XmlReader::from_reader(xml);
    let mut buffer = Vec::new();
    let mut cells = HashMap::new();
    let mut row_index = 0_u32;
    let mut col_index = 0_u32;

    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
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
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("invalid worksheet XML: {error}")),
        }
    }
    Ok(cells)
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
}

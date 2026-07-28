use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek};
use std::path::{Component, Path};
use std::time::{Duration, Instant};

use quick_xml::escape::resolve_xml_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader as XmlReader;
use wax_core::{Cell, CellType, CellValue, Document, DumpError, Sheet};
use wax_proto::ErrorCode;
use zip::ZipArchive;

pub const STUB_WARNING: &str =
    "stub reader: text via shared strings, formats, and dates are not implemented (W2)";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReaderOptions {
    pub max_cells: usize,
    pub timeout_ms: u64,
}

impl Default for ReaderOptions {
    fn default() -> Self {
        Self {
            max_cells: 200_000,
            timeout_ms: 30_000,
        }
    }
}

pub trait Reader {
    fn read(&self, path: &Path, options: ReaderOptions) -> Document;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StubReader;

impl Reader for StubReader {
    fn read(&self, path: &Path, options: ReaderOptions) -> Document {
        let file = path.to_string_lossy().into_owned();
        let started = Instant::now();
        let deadline = Duration::from_millis(options.timeout_ms);

        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("xlsx"))
        {
            return failure(
                file,
                ErrorCode::Unsupported,
                "stub reader supports .xlsx files only",
            );
        }

        if timed_out(started, deadline) {
            return failure(file, ErrorCode::Timeout, "xlsx parse timed out");
        }

        let input = match File::open(path) {
            Ok(input) => input,
            Err(error) => {
                return failure(
                    file,
                    ErrorCode::Internal,
                    format!("could not open input: {error}"),
                )
            }
        };
        let buffered = BufReader::new(input);
        let mut archive = match ZipArchive::new(buffered) {
            Ok(archive) => archive,
            Err(error) => {
                return failure(
                    file,
                    ErrorCode::BadZip,
                    format!("invalid xlsx zip: {error}"),
                )
            }
        };

        let workbook_xml = match read_zip_entry(&mut archive, "xl/workbook.xml") {
            Ok(xml) => xml,
            Err(message) => return failure(file, ErrorCode::BadZip, message),
        };
        let workbook_sheets = match parse_workbook(&workbook_xml, started, deadline) {
            Ok(sheets) => sheets,
            Err(ParseFailure::Timeout) => {
                return failure(file, ErrorCode::Timeout, "xlsx parse timed out")
            }
            Err(ParseFailure::Invalid(message)) => {
                return failure(file, ErrorCode::BadZip, message)
            }
        };

        let relationships =
            match read_optional_zip_entry(&mut archive, "xl/_rels/workbook.xml.rels") {
                Ok(Some(xml)) => match parse_relationships(&xml, started, deadline) {
                    Ok(relationships) => relationships,
                    Err(ParseFailure::Timeout) => {
                        return failure(file, ErrorCode::Timeout, "xlsx parse timed out")
                    }
                    Err(ParseFailure::Invalid(message)) => {
                        return failure(file, ErrorCode::BadZip, message)
                    }
                },
                Ok(None) => HashMap::new(),
                Err(message) => return failure(file, ErrorCode::BadZip, message),
            };

        let mut emitted_cells = 0_usize;
        let mut sheets = Vec::with_capacity(workbook_sheets.len());
        for (index, workbook_sheet) in workbook_sheets.into_iter().enumerate() {
            if timed_out(started, deadline) {
                return failure(file, ErrorCode::Timeout, "xlsx parse timed out");
            }

            let sheet_path = resolve_sheet_path(&workbook_sheet, index, &relationships);
            let sheet_xml = match read_zip_entry(&mut archive, &sheet_path) {
                Ok(xml) => xml,
                Err(message) => return failure(file, ErrorCode::BadZip, message),
            };
            let parsed = match parse_sheet(
                &sheet_xml,
                &mut emitted_cells,
                options.max_cells,
                started,
                deadline,
            ) {
                Ok(sheet) => sheet,
                Err(ParseFailure::Timeout) => {
                    return failure(file, ErrorCode::Timeout, "xlsx parse timed out")
                }
                Err(ParseFailure::Invalid(message)) => {
                    return failure(file, ErrorCode::BadZip, message)
                }
            };
            sheets.push(Sheet {
                name: workbook_sheet.name,
                index: index as u32,
                rows: parsed.rows,
                cols: parsed.cols,
                truncated: parsed.truncated,
                merges: Vec::new(),
                cells: parsed.cells,
            });
        }

        Document::success(
            env!("CARGO_PKG_VERSION"),
            file,
            sheets,
            vec![STUB_WARNING.to_owned()],
        )
    }
}

fn failure(file: String, code: ErrorCode, message: impl Into<String>) -> Document {
    Document::failure(
        env!("CARGO_PKG_VERSION"),
        file,
        DumpError {
            code: code.as_str().to_owned(),
            msg: message.into(),
        },
        vec![STUB_WARNING.to_owned()],
    )
}

fn timed_out(started: Instant, deadline: Duration) -> bool {
    started.elapsed() >= deadline
}

fn read_zip_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, String> {
    let mut entry = archive
        .by_name(name)
        .map_err(|error| format!("missing or unreadable {name}: {error}"))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read {name}: {error}"))?;
    Ok(bytes)
}

fn read_optional_zip_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Option<Vec<u8>>, String> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(format!("could not open {name}: {error}")),
    };
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read {name}: {error}"))?;
    Ok(Some(bytes))
}

#[derive(Debug)]
struct WorkbookSheet {
    name: String,
    sheet_id: Option<String>,
    relationship_id: Option<String>,
}

#[derive(Debug)]
enum ParseFailure {
    Timeout,
    Invalid(String),
}

fn parse_workbook(
    xml: &[u8],
    started: Instant,
    deadline: Duration,
) -> Result<Vec<WorkbookSheet>, ParseFailure> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    let mut buffer = Vec::new();
    let mut sheets = Vec::new();

    loop {
        if timed_out(started, deadline) {
            return Err(ParseFailure::Timeout);
        }
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element) | Event::Empty(element))
                if local_name(element.name().as_ref()) == b"sheet" =>
            {
                let name = attribute(&reader, &element, b"name")?.ok_or_else(|| {
                    ParseFailure::Invalid("workbook sheet is missing name".to_owned())
                })?;
                sheets.push(WorkbookSheet {
                    name,
                    sheet_id: attribute(&reader, &element, b"sheetId")?,
                    relationship_id: attribute(&reader, &element, b"r:id")?,
                });
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(ParseFailure::Invalid(format!(
                    "invalid xl/workbook.xml: {error}"
                )))
            }
        }
        buffer.clear();
    }
    Ok(sheets)
}

fn parse_relationships(
    xml: &[u8],
    started: Instant,
    deadline: Duration,
) -> Result<HashMap<String, String>, ParseFailure> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    let mut buffer = Vec::new();
    let mut relationships = HashMap::new();

    loop {
        if timed_out(started, deadline) {
            return Err(ParseFailure::Timeout);
        }
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element) | Event::Empty(element))
                if local_name(element.name().as_ref()) == b"Relationship" =>
            {
                if let (Some(id), Some(target)) = (
                    attribute(&reader, &element, b"Id")?,
                    attribute(&reader, &element, b"Target")?,
                ) {
                    relationships.insert(id, target);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(ParseFailure::Invalid(format!(
                    "invalid workbook relationships: {error}"
                )))
            }
        }
        buffer.clear();
    }
    Ok(relationships)
}

fn attribute(
    reader: &XmlReader<Cursor<&[u8]>>,
    element: &BytesStart<'_>,
    key: &[u8],
) -> Result<Option<String>, ParseFailure> {
    for attribute in element.attributes() {
        let attribute = attribute
            .map_err(|error| ParseFailure::Invalid(format!("invalid XML attribute: {error}")))?;
        if attribute.key.as_ref() == key {
            return attribute
                .decode_and_unescape_value(reader.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(|error| {
                    ParseFailure::Invalid(format!("invalid XML attribute value: {error}"))
                });
        }
    }
    Ok(None)
}

fn resolve_sheet_path(
    sheet: &WorkbookSheet,
    index: usize,
    relationships: &HashMap<String, String>,
) -> String {
    let target = sheet
        .relationship_id
        .as_ref()
        .and_then(|id| relationships.get(id));
    if let Some(target) = target {
        let target = Path::new(target);
        let joined = if target.is_absolute() {
            target.to_path_buf()
        } else {
            Path::new("xl").join(target)
        };
        let normalized = normalize_zip_path(&joined);
        if !normalized.is_empty() {
            return normalized;
        }
    }

    let suffix = sheet
        .sheet_id
        .as_deref()
        .map(str::to_owned)
        .unwrap_or_else(|| (index + 1).to_string());
    format!("xl/worksheets/sheet{suffix}.xml")
}

fn normalize_zip_path(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::ParentDir => {
                parts.pop();
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    parts.join("/")
}

#[derive(Debug)]
struct ParsedSheet {
    rows: u32,
    cols: u32,
    truncated: bool,
    cells: Vec<Cell>,
}

#[derive(Debug)]
struct CellBuilder {
    row: u32,
    col: u32,
    kind: Option<String>,
    numeric_text: String,
    inline_text: String,
}

fn parse_sheet(
    xml: &[u8],
    emitted_cells: &mut usize,
    max_cells: usize,
    started: Instant,
    deadline: Duration,
) -> Result<ParsedSheet, ParseFailure> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut dimension = None;
    let mut seen_rows = 0_u32;
    let mut seen_cols = 0_u32;
    let mut current_row = 1_u32;
    let mut next_col = 0_u32;
    let mut current_cell: Option<CellBuilder> = None;
    let mut in_value = false;
    let mut inline_depth = 0_u32;
    let mut in_inline_text = false;
    let mut cells = Vec::new();
    let mut truncated = false;

    loop {
        if timed_out(started, deadline) {
            return Err(ParseFailure::Timeout);
        }
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) => match local_name(element.name().as_ref()) {
                b"dimension" => {
                    if let Some(reference) = attribute(&reader, &element, b"ref")? {
                        dimension = parse_dimension(&reference);
                    }
                }
                b"row" => {
                    if let Some(row) = attribute(&reader, &element, b"r")? {
                        current_row = row.parse::<u32>().unwrap_or(current_row);
                    }
                    next_col = 0;
                }
                b"c" => {
                    let coordinate = attribute(&reader, &element, b"r")?
                        .and_then(|reference| parse_cell_reference(&reference))
                        .unwrap_or_else(|| (current_row.saturating_sub(1), next_col));
                    next_col = coordinate.1.saturating_add(1);
                    seen_rows = seen_rows.max(coordinate.0.saturating_add(1));
                    seen_cols = seen_cols.max(coordinate.1.saturating_add(1));
                    current_cell = Some(CellBuilder {
                        row: coordinate.0,
                        col: coordinate.1,
                        kind: attribute(&reader, &element, b"t")?,
                        numeric_text: String::new(),
                        inline_text: String::new(),
                    });
                }
                b"v" if current_cell.is_some() => in_value = true,
                b"is" if current_cell.is_some() => inline_depth += 1,
                b"t" if current_cell.is_some() && inline_depth > 0 => in_inline_text = true,
                _ => {}
            },
            Ok(Event::Empty(element)) => match local_name(element.name().as_ref()) {
                b"dimension" => {
                    if let Some(reference) = attribute(&reader, &element, b"ref")? {
                        dimension = parse_dimension(&reference);
                    }
                }
                b"c" => {
                    let coordinate = attribute(&reader, &element, b"r")?
                        .and_then(|reference| parse_cell_reference(&reference))
                        .unwrap_or_else(|| (current_row.saturating_sub(1), next_col));
                    next_col = coordinate.1.saturating_add(1);
                    seen_rows = seen_rows.max(coordinate.0.saturating_add(1));
                    seen_cols = seen_cols.max(coordinate.1.saturating_add(1));
                }
                _ => {}
            },
            Ok(Event::Text(text)) => {
                if let Some(cell) = current_cell.as_mut() {
                    let decoded = text
                        .xml_content()
                        .map_err(|error| {
                            ParseFailure::Invalid(format!("invalid worksheet text: {error}"))
                        })?
                        .into_owned();
                    if in_value {
                        cell.numeric_text.push_str(&decoded);
                    }
                    if in_inline_text {
                        cell.inline_text.push_str(&decoded);
                    }
                }
            }
            Ok(Event::CData(text)) => {
                if let Some(cell) = current_cell.as_mut() {
                    let decoded = text
                        .xml_content()
                        .map_err(|error| {
                            ParseFailure::Invalid(format!("invalid worksheet CDATA: {error}"))
                        })?
                        .into_owned();
                    if in_value {
                        cell.numeric_text.push_str(&decoded);
                    }
                    if in_inline_text {
                        cell.inline_text.push_str(&decoded);
                    }
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(cell) = current_cell.as_mut() {
                    let decoded = reference.xml_content().map_err(|error| {
                        ParseFailure::Invalid(format!("invalid worksheet entity: {error}"))
                    })?;
                    let resolved = if let Some(character) =
                        reference.resolve_char_ref().map_err(|error| {
                            ParseFailure::Invalid(format!("invalid worksheet entity: {error}"))
                        })? {
                        character.to_string()
                    } else {
                        resolve_xml_entity(&decoded)
                            .ok_or_else(|| {
                                ParseFailure::Invalid(format!(
                                    "unknown worksheet entity: &{decoded};"
                                ))
                            })?
                            .to_owned()
                    };
                    if in_value {
                        cell.numeric_text.push_str(&resolved);
                    }
                    if in_inline_text {
                        cell.inline_text.push_str(&resolved);
                    }
                }
            }
            Ok(Event::End(element)) => match local_name(element.name().as_ref()) {
                b"v" => in_value = false,
                b"t" => in_inline_text = false,
                b"is" => inline_depth = inline_depth.saturating_sub(1),
                b"c" => {
                    if let Some(builder) = current_cell.take() {
                        if let Some(cell) = finish_cell(builder) {
                            if *emitted_cells < max_cells {
                                cells.push(cell);
                                *emitted_cells += 1;
                            } else {
                                truncated = true;
                            }
                        }
                    }
                    in_value = false;
                    inline_depth = 0;
                    in_inline_text = false;
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(ParseFailure::Invalid(format!(
                    "invalid worksheet XML: {error}"
                )))
            }
        }
        buffer.clear();
    }

    cells.sort_by_key(|cell| (cell.r, cell.c));
    let (rows, cols) = dimension.unwrap_or((seen_rows, seen_cols));
    Ok(ParsedSheet {
        rows,
        cols,
        truncated,
        cells,
    })
}

fn finish_cell(builder: CellBuilder) -> Option<Cell> {
    let (cell_type, value) = match builder.kind.as_deref() {
        Some("inlineStr") if !builder.inline_text.is_empty() => {
            (CellType::S, CellValue::Text(builder.inline_text))
        }
        Some("inlineStr") => return None,
        None | Some("n") => {
            let number = builder.numeric_text.trim().parse::<f64>().ok()?;
            if !number.is_finite() {
                return None;
            }
            (CellType::N, CellValue::Number(number))
        }
        _ => return None,
    };
    Some(Cell {
        r: builder.row,
        c: builder.col,
        t: cell_type,
        v: Some(value),
        d: None,
        f: None,
        fmt: None,
    })
}

fn parse_dimension(reference: &str) -> Option<(u32, u32)> {
    let end = reference.rsplit(':').next()?;
    let (row, col) = parse_cell_reference(end)?;
    Some((row.saturating_add(1), col.saturating_add(1)))
}

fn parse_cell_reference(reference: &str) -> Option<(u32, u32)> {
    let mut col = 0_u32;
    let mut saw_column = false;
    let mut row_text = String::new();

    for byte in reference.bytes().filter(|byte| *byte != b'$') {
        if byte.is_ascii_alphabetic() && row_text.is_empty() {
            saw_column = true;
            col = col
                .checked_mul(26)?
                .checked_add(u32::from(byte.to_ascii_uppercase() - b'A' + 1))?;
        } else if byte.is_ascii_digit() {
            row_text.push(char::from(byte));
        } else {
            return None;
        }
    }

    if !saw_column || row_text.is_empty() {
        return None;
    }
    let row = row_text.parse::<u32>().ok()?;
    if row == 0 || col == 0 {
        return None;
    }
    Some((row - 1, col - 1))
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn parses_absolute_and_mixed_case_cell_references() {
        assert_eq!(parse_cell_reference("$a$1"), Some((0, 0)));
        assert_eq!(
            parse_cell_reference("XFD1048576"),
            Some((1_048_575, 16_383))
        );
        assert_eq!(parse_cell_reference("1A"), None);
    }

    #[test]
    fn parses_range_extent_from_bottom_right_cell() {
        assert_eq!(parse_dimension("A1:C12"), Some((12, 3)));
        assert_eq!(parse_dimension("B7"), Some((7, 2)));
    }
}

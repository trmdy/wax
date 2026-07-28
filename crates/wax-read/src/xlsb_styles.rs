use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::ZipArchive;
use zip::ZipWriter;

use super::{
    builtin_format, explicit_format, parse_relationships, read_part, read_part_optional,
    relationships_path, resolve_part, zip_lookup, zip_part_key,
};

#[derive(Default)]
pub(super) struct XlsbStyleSupplement {
    sheets: HashMap<String, HashMap<(u32, u32), XlsbCellMetadata>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct XlsbCellMetadata {
    pub(super) format: Option<String>,
    pub(super) cached_error: Option<u8>,
    pub(super) cached_empty_string: bool,
}

impl XlsbStyleSupplement {
    pub(super) fn read(path: &Path) -> Result<Self, String> {
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
            .unwrap_or_else(|| "xl/workbook.bin".to_owned());
        let workbook = read_part(&mut archive, &lookup, &workbook_path)?;
        let relationships_xml =
            read_part(&mut archive, &lookup, &relationships_path(&workbook_path))?;
        let relationships = parse_relationships(&relationships_xml)?;
        let by_id = relationships
            .iter()
            .map(|relationship| (relationship.id.as_str(), relationship.target.as_str()))
            .collect::<HashMap<_, _>>();

        let styles_path = relationships
            .iter()
            .find(|relationship| relationship.kind.ends_with("/styles"))
            .map(|relationship| resolve_part(&workbook_path, &relationship.target))
            .unwrap_or_else(|| resolve_part(&workbook_path, "styles.bin"));
        let formats = match read_part_optional(&mut archive, &lookup, &styles_path)? {
            Some(styles) => parse_styles(&styles)?,
            None => Vec::new(),
        };

        let mut sheets = HashMap::new();
        for sheet in parse_workbook_sheets(&workbook)? {
            let Some(target) = by_id.get(sheet.relationship_id.as_str()) else {
                continue;
            };
            let sheet_path = resolve_part(&workbook_path, target);
            let Some(bytes) = read_part_optional(&mut archive, &lookup, &sheet_path)? else {
                continue;
            };
            sheets.insert(sheet.name, parse_sheet_styles(&bytes, &formats)?);
        }
        Ok(Self { sheets })
    }

    pub(super) fn cell(&self, sheet: &str, position: (u32, u32)) -> Option<&str> {
        self.sheets.get(sheet)?.get(&position)?.format.as_deref()
    }

    pub(super) fn cells(&self, sheet: &str) -> Option<&HashMap<(u32, u32), XlsbCellMetadata>> {
        self.sheets.get(sheet)
    }
}

pub(super) fn normalize_legacy_bundle_workbook(
    path: &Path,
) -> Result<Option<Cursor<Vec<u8>>>, String> {
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
        .unwrap_or_else(|| "xl/workbook.bin".to_owned());
    let workbook = read_part(&mut archive, &lookup, &workbook_path)?;
    let Some(normalized_workbook) = rewrite_legacy_bundle_sheets(&workbook)? else {
        return Ok(None);
    };

    let workbook_key = zip_part_key(&workbook_path);
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
            .write_all(if zip_part_key(&name) == workbook_key {
                &normalized_workbook
            } else {
                &bytes
            })
            .map_err(|error| format!("could not normalize {name}: {error}"))?;
    }
    let mut reader = writer
        .finish()
        .map_err(|error| format!("could not finish normalized package: {error}"))?;
    reader.set_position(0);
    Ok(Some(reader))
}

struct BinarySheet {
    name: String,
    relationship_id: String,
}

fn parse_workbook_sheets(bytes: &[u8]) -> Result<Vec<BinarySheet>, String> {
    let mut sheets = Vec::new();
    for record in BinaryRecordIter::new(bytes) {
        let record = record?;
        if record.kind != 0x009C || record.data.len() < 16 {
            continue;
        }
        sheets.push(parse_bundle_sheet(record.data)?.0);
    }
    Ok(sheets)
}

fn parse_bundle_sheet(data: &[u8]) -> Result<(BinarySheet, usize), String> {
    // BrtBundleSh normally stores its relationship string after eight fixed
    // bytes. Excel 2007 Beta 2 inserted a four-byte tab id first. Accept both
    // layouts only when the two strings consume the record exactly.
    for string_offset in [8_usize, 12] {
        let Some(tail) = data.get(string_offset..) else {
            continue;
        };
        let Ok(Some((relationship_id, relationship_len))) = parse_nullable_wide_string(tail) else {
            continue;
        };
        let Some(name_tail) = tail.get(relationship_len..) else {
            continue;
        };
        let Ok((name, name_len)) = parse_wide_string(name_tail) else {
            continue;
        };
        if relationship_len + name_len == tail.len() {
            return Ok((
                BinarySheet {
                    name,
                    relationship_id,
                },
                string_offset,
            ));
        }
    }
    Err("invalid BrtBundleSh strings".to_owned())
}

fn rewrite_legacy_bundle_sheets(bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut rewritten = Vec::with_capacity(bytes.len());
    let mut changed = false;
    for record in BinaryRecordIter::new(bytes) {
        let record = record?;
        if record.kind == 0x009C && parse_bundle_sheet(record.data)?.1 == 12 {
            let mut normalized = Vec::with_capacity(record.data.len() - 4);
            normalized.extend_from_slice(&record.data[..8]);
            normalized.extend_from_slice(&record.data[12..]);
            push_binary_record(&mut rewritten, record.kind, &normalized);
            changed = true;
        } else {
            push_binary_record(&mut rewritten, record.kind, record.data);
        }
    }
    Ok(changed.then_some(rewritten))
}

fn parse_styles(bytes: &[u8]) -> Result<Vec<Option<String>>, String> {
    let mut custom = HashMap::<u16, String>::new();
    let mut formats = Vec::new();
    let mut in_cell_xfs = false;

    for record in BinaryRecordIter::new(bytes) {
        let record = record?;
        match record.kind {
            0x002C if record.data.len() >= 6 => {
                let id = u16::from_le_bytes([record.data[0], record.data[1]]);
                let (code, _) = parse_wide_string(&record.data[2..])?;
                custom.insert(id, code);
            }
            0x0269 => in_cell_xfs = true,
            0x026A => in_cell_xfs = false,
            0x002F if in_cell_xfs && record.data.len() >= 4 => {
                let id = u16::from_le_bytes([record.data[2], record.data[3]]);
                let format = custom
                    .get(&id)
                    .cloned()
                    .or_else(|| builtin_format(u32::from(id)).map(str::to_owned))
                    .and_then(|code| explicit_format(Some(&code)));
                formats.push(format);
            }
            _ => {}
        }
    }
    Ok(formats)
}

fn parse_sheet_styles(
    bytes: &[u8],
    formats: &[Option<String>],
) -> Result<HashMap<(u32, u32), XlsbCellMetadata>, String> {
    let mut styles = HashMap::new();
    let mut row = 0_u32;
    for record in BinaryRecordIter::new(bytes) {
        let record = record?;
        match record.kind {
            0x0000 if record.data.len() >= 4 => {
                row = u32::from_le_bytes(record.data[..4].try_into().expect("four bytes checked"));
            }
            0x0001..=0x000B if record.data.len() >= 8 => {
                let col =
                    u32::from_le_bytes(record.data[..4].try_into().expect("four bytes checked"));
                let style = u32::from_le_bytes([record.data[4], record.data[5], record.data[6], 0])
                    as usize;
                let format = formats.get(style).cloned().flatten();
                let cached_error = (record.kind == 0x000B)
                    .then(|| record.data.get(8).copied())
                    .flatten();
                let cached_empty_string =
                    record.kind == 0x0008 && record.data.get(8..12) == Some(&0_u32.to_le_bytes());
                if format.is_some() || cached_error.is_some() || cached_empty_string {
                    styles.insert(
                        (row, col),
                        XlsbCellMetadata {
                            format,
                            cached_error,
                            cached_empty_string,
                        },
                    );
                }
            }
            _ => {}
        }
    }
    Ok(styles)
}

fn parse_nullable_wide_string(bytes: &[u8]) -> Result<Option<(String, usize)>, String> {
    if bytes.len() < 4 {
        return Err("truncated nullable wide string".to_owned());
    }
    if u32::from_le_bytes(bytes[..4].try_into().expect("four bytes checked")) == u32::MAX {
        return Ok(None);
    }
    parse_wide_string(bytes).map(Some)
}

fn parse_wide_string(bytes: &[u8]) -> Result<(String, usize), String> {
    if bytes.len() < 4 {
        return Err("truncated wide string length".to_owned());
    }
    let count = u32::from_le_bytes(bytes[..4].try_into().expect("four bytes checked")) as usize;
    let byte_len = count
        .checked_mul(2)
        .ok_or_else(|| "wide string length overflow".to_owned())?;
    let end = 4_usize
        .checked_add(byte_len)
        .ok_or_else(|| "wide string length overflow".to_owned())?;
    let encoded = bytes
        .get(4..end)
        .ok_or_else(|| "truncated wide string".to_owned())?;
    let units = encoded
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let string =
        String::from_utf16(&units).map_err(|error| format!("invalid UTF-16 string: {error}"))?;
    Ok((string, end))
}

struct BinaryRecord<'a> {
    kind: u16,
    data: &'a [u8],
}

struct BinaryRecordIter<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> BinaryRecordIter<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
}

fn push_binary_record(target: &mut Vec<u8>, kind: u16, data: &[u8]) {
    if kind < 0x80 {
        target.push(kind as u8);
    } else {
        target.push((kind as u8 & 0x7F) | 0x80);
        target.push((kind >> 7) as u8);
    }
    let mut len = data.len();
    loop {
        let mut byte = (len & 0x7F) as u8;
        len >>= 7;
        if len != 0 {
            byte |= 0x80;
        }
        target.push(byte);
        if len == 0 {
            break;
        }
    }
    target.extend_from_slice(data);
}

impl<'a> Iterator for BinaryRecordIter<'a> {
    type Item = Result<BinaryRecord<'a>, String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset == self.bytes.len() {
            return None;
        }
        Some(self.next_record())
    }
}

impl<'a> BinaryRecordIter<'a> {
    fn next_record(&mut self) -> Result<BinaryRecord<'a>, String> {
        let first = *self
            .bytes
            .get(self.offset)
            .ok_or_else(|| "truncated XLSB record id".to_owned())?;
        self.offset += 1;
        let kind = if first & 0x80 != 0 {
            let second = *self
                .bytes
                .get(self.offset)
                .ok_or_else(|| "truncated XLSB record id".to_owned())?;
            self.offset += 1;
            u16::from(first & 0x7F) | (u16::from(second & 0x7F) << 7)
        } else {
            u16::from(first)
        };

        let mut len = 0_usize;
        let mut terminated = false;
        for shift in [0, 7, 14, 21] {
            let byte = *self
                .bytes
                .get(self.offset)
                .ok_or_else(|| format!("truncated XLSB record 0x{kind:04X} length"))?;
            self.offset += 1;
            len |= usize::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                terminated = true;
                break;
            }
        }
        if !terminated {
            return Err(format!("invalid XLSB record 0x{kind:04X} length"));
        }
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| format!("XLSB record 0x{kind:04X} length overflow"))?;
        let data = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| format!("truncated XLSB record 0x{kind:04X}"))?;
        self.offset = end;
        Ok(BinaryRecord { kind, data })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_brt_fmt_xf_and_cell_style_indices() {
        let code = "#,##0.000";
        let mut styles = Vec::new();
        push_record(&mut styles, 0x0267, &1_u32.to_le_bytes());
        let mut format = 164_u16.to_le_bytes().to_vec();
        push_wide_string(&mut format, code);
        push_record(&mut styles, 0x002C, &format);
        push_record(&mut styles, 0x0268, &[]);
        push_record(&mut styles, 0x0269, &2_u32.to_le_bytes());
        push_record(&mut styles, 0x002F, &[0, 0, 0, 0]);
        push_record(&mut styles, 0x002F, &[0, 0, 164, 0]);
        push_record(&mut styles, 0x026A, &[]);

        let formats = parse_styles(&styles).unwrap();
        assert_eq!(formats, vec![None, Some(code.to_owned())]);

        let mut sheet = Vec::new();
        push_record(&mut sheet, 0x0000, &4_u32.to_le_bytes());
        let mut cell = 2_u32.to_le_bytes().to_vec();
        cell.extend_from_slice(&[1, 0, 0, 0]);
        cell.extend_from_slice(&42.5_f64.to_le_bytes());
        push_record(&mut sheet, 0x0005, &cell);
        let cells = parse_sheet_styles(&sheet, &formats).unwrap();
        assert_eq!(
            cells.get(&(4, 2)),
            Some(&XlsbCellMetadata {
                format: Some(code.to_owned()),
                cached_error: None,
                cached_empty_string: false,
            })
        );
    }

    #[test]
    fn preserves_cached_formula_errors_skipped_by_calamine() {
        let mut sheet = Vec::new();
        push_record(&mut sheet, 0x0000, &6_u32.to_le_bytes());
        let mut formula_error = 2_u32.to_le_bytes().to_vec();
        formula_error.extend_from_slice(&[0, 0, 0, 0]);
        formula_error.push(0x07);
        formula_error.extend_from_slice(&[0, 0]);
        push_record(&mut sheet, 0x000B, &formula_error);

        let cells = parse_sheet_styles(&sheet, &[]).unwrap();
        assert_eq!(
            cells.get(&(6, 2)),
            Some(&XlsbCellMetadata {
                format: None,
                cached_error: Some(0x07),
                cached_empty_string: false,
            })
        );
    }

    #[test]
    fn preserves_empty_cached_formula_strings_skipped_by_sparse_ranges() {
        let mut sheet = Vec::new();
        push_record(&mut sheet, 0x0000, &6_u32.to_le_bytes());
        let mut formula_string = 2_u32.to_le_bytes().to_vec();
        formula_string.extend_from_slice(&[0, 0, 0, 0]);
        formula_string.extend_from_slice(&0_u32.to_le_bytes());
        push_record(&mut sheet, 0x0008, &formula_string);

        let cells = parse_sheet_styles(&sheet, &[]).unwrap();
        assert_eq!(
            cells.get(&(6, 2)),
            Some(&XlsbCellMetadata {
                format: None,
                cached_error: None,
                cached_empty_string: true,
            })
        );
    }

    #[test]
    fn parses_bundle_sheet_relationship_and_name() {
        let mut payload = vec![0; 8];
        push_wide_string(&mut payload, "rId3");
        push_wide_string(&mut payload, "Costs");
        let mut workbook = Vec::new();
        push_record(&mut workbook, 0x009C, &payload);
        let sheets = parse_workbook_sheets(&workbook).unwrap();
        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].relationship_id, "rId3");
        assert_eq!(sheets[0].name, "Costs");
    }

    #[test]
    fn rewrites_excel_2007_beta_bundle_sheet_layout() {
        let mut payload = vec![0; 8];
        payload.extend_from_slice(&1_u32.to_le_bytes());
        push_wide_string(&mut payload, "rId1");
        push_wide_string(&mut payload, "Sheet1");
        let mut workbook = Vec::new();
        push_record(&mut workbook, 0x009C, &payload);

        let rewritten = rewrite_legacy_bundle_sheets(&workbook)
            .unwrap()
            .expect("legacy record should be rewritten");
        let sheets = parse_workbook_sheets(&rewritten).unwrap();
        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].relationship_id, "rId1");
        assert_eq!(sheets[0].name, "Sheet1");
        assert_eq!(rewritten.len(), workbook.len() - 4);
    }

    fn push_record(target: &mut Vec<u8>, kind: u16, data: &[u8]) {
        push_binary_record(target, kind, data);
    }

    fn push_wide_string(target: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        target.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            target.extend_from_slice(&unit.to_le_bytes());
        }
    }
}

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;

use codepage::to_encoding;
use encoding_rs::{Encoding, WINDOWS_1252};

use super::{
    builtin_format, explicit_format, ColumnDeclaration, RowDeclaration, SheetDefaults, SheetSizes,
    SheetView,
};

type CellPosition = (u32, u32);
type ParsedSheetStyles = (
    HashMap<CellPosition, String>,
    HashSet<CellPosition>,
    SheetSizes,
    SheetView,
);

#[derive(Default)]
pub(super) struct XlsStyleSupplement {
    sheets: Vec<HashMap<(u32, u32), String>>,
    empty_formula_cells: Vec<HashSet<(u32, u32)>>,
    sizes: Vec<SheetSizes>,
    views: Vec<SheetView>,
}

impl XlsStyleSupplement {
    pub(super) fn read(path: &Path) -> Result<Self, String> {
        let mut compound =
            cfb::open(path).map_err(|error| format!("could not open OLE container: {error}"))?;
        let mut stream = ["/Workbook", "/Book", "/WORKBOOK", "/BOOK"]
            .into_iter()
            .find_map(|name| compound.open_stream(name).ok())
            .ok_or_else(|| "OLE container has no Workbook stream".to_owned())?;
        let mut bytes = Vec::new();
        stream
            .read_to_end(&mut bytes)
            .map_err(|error| format!("could not read Workbook stream: {error}"))?;
        parse_workbook_stream(&bytes)
    }

    pub(super) fn cell(&self, sheet_index: usize, position: (u32, u32)) -> Option<&str> {
        self.sheets
            .get(sheet_index)?
            .get(&position)
            .map(String::as_str)
    }

    pub(super) fn empty_formula_cells(
        &self,
        sheet_index: usize,
    ) -> impl Iterator<Item = &(u32, u32)> {
        self.empty_formula_cells
            .get(sheet_index)
            .into_iter()
            .flatten()
    }

    pub(super) fn sizes(&self, sheet_index: usize) -> Option<&SheetSizes> {
        self.sizes.get(sheet_index)
    }

    pub(super) fn sheet_view(&self, sheet_index: usize) -> SheetView {
        self.views.get(sheet_index).copied().unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BiffVersion {
    Pre8,
    Biff8,
}

fn parse_workbook_stream(stream: &[u8]) -> Result<XlsStyleSupplement, String> {
    let mut formats = HashMap::<u16, String>::new();
    let mut xfs = Vec::<u16>::new();
    let mut sheet_offsets = Vec::<usize>::new();
    let mut biff = BiffVersion::Biff8;
    let mut encoding = WINDOWS_1252;

    let mut records = BiffRecordIter::new(stream);
    while let Some(record) = records.next_record()? {
        match record.kind {
            0x0809 => biff = parse_biff_version(record.data),
            0x0042 if record.data.len() >= 2 => {
                let codepage = u16::from_le_bytes([record.data[0], record.data[1]]);
                encoding = to_encoding(codepage).unwrap_or(WINDOWS_1252);
            }
            0x041E => {
                if let Some((id, code)) = parse_format(record.data, biff, encoding) {
                    formats.insert(id, code);
                }
            }
            0x00E0 if record.data.len() >= 4 => {
                xfs.push(u16::from_le_bytes([record.data[2], record.data[3]]));
            }
            0x0085 if record.data.len() >= 6 => {
                sheet_offsets.push(u32::from_le_bytes(
                    record.data[..4].try_into().expect("four bytes checked"),
                ) as usize);
            }
            0x000A => break,
            _ => {}
        }
    }

    let xf_formats = xfs
        .into_iter()
        .map(|id| {
            formats
                .get(&id)
                .cloned()
                .or_else(|| builtin_format(u32::from(id)).map(str::to_owned))
                .and_then(|code| explicit_format(Some(&code)))
        })
        .collect::<Vec<_>>();
    let mut sheets = Vec::with_capacity(sheet_offsets.len());
    let mut empty_formula_cells = Vec::with_capacity(sheet_offsets.len());
    let mut sizes = Vec::with_capacity(sheet_offsets.len());
    let mut views = Vec::with_capacity(sheet_offsets.len());
    for offset in sheet_offsets {
        if offset >= stream.len() {
            sheets.push(HashMap::new());
            empty_formula_cells.push(HashSet::new());
            sizes.push(SheetSizes::default());
            views.push(SheetView::default());
            continue;
        }
        let (styles, empty_formulas, sheet_sizes, view) =
            parse_sheet_styles(&stream[offset..], &xf_formats)?;
        sheets.push(styles);
        empty_formula_cells.push(empty_formulas);
        sizes.push(sheet_sizes);
        views.push(view);
    }
    Ok(XlsStyleSupplement {
        sheets,
        empty_formula_cells,
        sizes,
        views,
    })
}

fn parse_sheet_styles(
    stream: &[u8],
    xf_formats: &[Option<String>],
) -> Result<ParsedSheetStyles, String> {
    let mut styles = HashMap::new();
    let mut empty_formula_cells = HashSet::new();
    let mut pending_string_formula = None;
    // Raw ROW records as (row, height in twips, fUnsynced); filtered into
    // declarations once the sheet's default height is known.
    let mut raw_rows = Vec::<(u32, u16, bool)>::new();
    let mut columns = Vec::<ColumnDeclaration>::new();
    let mut default_row_twips = None::<u16>;
    let mut default_col_chars = None::<u16>;
    let mut standard_width_256 = None::<u16>;
    let mut frozen = false;
    let mut pane = None::<(u32, u32)>;
    let mut records = BiffRecordIter::new(stream);
    while let Some(record) = records.next_record()? {
        // A string FORMULA may be followed by SHRFMLA (0x04BC), ARRAY
        // (0x0221), or TABLE (0x0236) before its STRING record; those must
        // not clear the pending position (MS-XLS 2.4.127).
        if !matches!(record.kind, 0x0207 | 0x003C | 0x04BC | 0x0221 | 0x0236) {
            pending_string_formula = None;
        }
        match record.kind {
            // Blank, Label, BoolErr, Number, RString, RK, LabelSst.
            0x0201 | 0x0203 | 0x0204 | 0x0205 | 0x00D6 | 0x027E | 0x00FD
                if record.data.len() >= 6 =>
            {
                let row = u16::from_le_bytes([record.data[0], record.data[1]]) as u32;
                let col = u16::from_le_bytes([record.data[2], record.data[3]]) as u32;
                let xf = u16::from_le_bytes([record.data[4], record.data[5]]) as usize;
                insert_style(&mut styles, (row, col), xf, xf_formats);
            }
            // Formula cached results use a sentinel. 0x03 is an immediate
            // empty string; 0x00 means the following STRING record carries
            // the cached text and may itself be empty.
            0x0006 if record.data.len() >= 14 => {
                let row = u16::from_le_bytes([record.data[0], record.data[1]]) as u32;
                let col = u16::from_le_bytes([record.data[2], record.data[3]]) as u32;
                let position = (row, col);
                let xf = u16::from_le_bytes([record.data[4], record.data[5]]) as usize;
                insert_style(&mut styles, position, xf, xf_formats);
                let cached = &record.data[6..14];
                if cached[6..] == [0xFF, 0xFF] {
                    match cached[0] {
                        0x03 => {
                            empty_formula_cells.insert(position);
                        }
                        0x00 => pending_string_formula = Some(position),
                        _ => {}
                    }
                }
            }
            0x0207 => {
                if record.data.get(..2) == Some(&[0, 0]) {
                    if let Some(position) = pending_string_formula.take() {
                        empty_formula_cells.insert(position);
                    }
                } else {
                    pending_string_formula = None;
                }
            }
            // Row ([MS-XLS] 2.4.221): rw, colMic, colMac, miyRw (twips),
            // then two reserved shorts and a flag block whose byte 12
            // carries fUnsynced (0x40, manually sized).
            0x0208 if record.data.len() >= 8 => {
                let row = u16::from_le_bytes([record.data[0], record.data[1]]) as u32;
                let twips = u16::from_le_bytes([record.data[6], record.data[7]]) & 0x7FFF;
                let unsynced = record.data.get(12).is_some_and(|flags| flags & 0x40 != 0);
                raw_rows.push((row, twips, unsynced));
            }
            // DefaultRowHeight ([MS-XLS] 2.4.87): flags, then miyRw in twips.
            0x0225 if record.data.len() >= 4 => {
                default_row_twips = Some(u16::from_le_bytes([record.data[2], record.data[3]]));
            }
            // DefColWidth ([MS-XLS] 2.4.89): whole character units, same
            // semantics as OOXML baseColWidth.
            0x0055 if record.data.len() >= 2 => {
                default_col_chars = Some(u16::from_le_bytes([record.data[0], record.data[1]]));
            }
            // Standardwidth ([MS-XLS] 2.4.319): 1/256 character units,
            // padding included; overrides DefColWidth when present.
            0x0099 if record.data.len() >= 2 => {
                standard_width_256 = Some(u16::from_le_bytes([record.data[0], record.data[1]]));
            }
            // ColInfo ([MS-XLS] 2.4.53): colFirst, colLast, coldx in 1/256
            // character units. Written only for non-default columns, so
            // every record is an explicit declaration (1-based min/max to
            // match the OOXML declaration shape).
            0x007D if record.data.len() >= 6 => {
                let first = u16::from_le_bytes([record.data[0], record.data[1]]) as u32;
                let last = u16::from_le_bytes([record.data[2], record.data[3]]) as u32;
                let coldx = u16::from_le_bytes([record.data[4], record.data[5]]);
                if last >= first {
                    columns.push(ColumnDeclaration {
                        min: first.saturating_add(1),
                        max: last.saturating_add(1),
                        width: f64::from(coldx) / 256.0,
                    });
                }
            }
            // Window2 ([MS-XLS] 2.4.345): fFrozen is bit 3. Pane split
            // positions are counts only in this mode; ordinary split panes
            // contain display units and intentionally report zero.
            0x023E if record.data.len() >= 2 => {
                let flags = u16::from_le_bytes([record.data[0], record.data[1]]);
                frozen = flags & 0x0008 != 0;
            }
            // Pane ([MS-XLS] 2.4.189): x is columns and y is rows when
            // Window2 marks the view frozen.
            0x0041 if record.data.len() >= 4 => {
                pane = Some((
                    u16::from_le_bytes([record.data[0], record.data[1]]) as u32,
                    u16::from_le_bytes([record.data[2], record.data[3]]) as u32,
                ));
            }
            // MulRk: row, first column, repeated (XF, RK), last column.
            0x00BD if record.data.len() >= 12 => {
                let row = u16::from_le_bytes([record.data[0], record.data[1]]) as u32;
                let first_col = u16::from_le_bytes([record.data[2], record.data[3]]) as u32;
                let entries = &record.data[4..record.data.len() - 2];
                for (index, entry) in entries.chunks_exact(6).enumerate() {
                    let xf = u16::from_le_bytes([entry[0], entry[1]]) as usize;
                    insert_style(
                        &mut styles,
                        (row, first_col.saturating_add(index as u32)),
                        xf,
                        xf_formats,
                    );
                }
            }
            0x000A => break,
            _ => {}
        }
    }

    // Excel writes a Row record for every populated row with its actual
    // height; only manually sized rows (fUnsynced) or rows differing from
    // the sheet's declared default are size declarations in wax's model
    // (mirroring the xlsx rule: a declared height is the rendered height).
    let rows = raw_rows
        .into_iter()
        .filter(|(_, twips, unsynced)| {
            *unsynced || default_row_twips.is_some_and(|default| *twips != default)
        })
        .map(|(row, twips, _)| RowDeclaration {
            row,
            height: f64::from(twips) / 20.0,
        })
        .collect();
    let defaults = SheetDefaults {
        row_height: default_row_twips.map(|twips| f64::from(twips) / 20.0),
        col_width: standard_width_256
            .map(|width| f64::from(width) / 256.0)
            .or_else(|| default_col_chars.map(|chars| f64::from(chars) + 5.0 / 7.0)),
    };
    Ok((
        styles,
        empty_formula_cells,
        SheetSizes {
            rows,
            columns,
            defaults,
        },
        if frozen {
            let (frozen_cols, frozen_rows) = pane.unwrap_or_default();
            SheetView {
                frozen_rows,
                frozen_cols,
            }
        } else {
            SheetView::default()
        },
    ))
}

fn insert_style(
    styles: &mut HashMap<(u32, u32), String>,
    position: (u32, u32),
    xf: usize,
    xf_formats: &[Option<String>],
) {
    if let Some(Some(code)) = xf_formats.get(xf) {
        styles.insert(position, code.clone());
    }
}

fn parse_biff_version(data: &[u8]) -> BiffVersion {
    if data.len() >= 2 && u16::from_le_bytes([data[0], data[1]]) == 0x0600 {
        BiffVersion::Biff8
    } else {
        BiffVersion::Pre8
    }
}

fn parse_format(
    data: &[u8],
    biff: BiffVersion,
    encoding: &'static Encoding,
) -> Option<(u16, String)> {
    if data.len() < 4 {
        return None;
    }
    let id = u16::from_le_bytes([data[0], data[1]]);
    let count = u16::from_le_bytes([data[2], data[3]]) as usize;
    let code = match biff {
        BiffVersion::Pre8 => {
            let bytes = data.get(4..4_usize.saturating_add(count))?;
            encoding.decode(bytes).0.into_owned()
        }
        BiffVersion::Biff8 => {
            let flags = *data.get(4)?;
            let mut offset = 5_usize;
            if flags & 0x08 != 0 {
                offset = offset.checked_add(2)?;
            }
            if flags & 0x04 != 0 {
                offset = offset.checked_add(4)?;
            }
            if flags & 0x01 != 0 {
                let byte_len = count.checked_mul(2)?;
                decode_utf16le(data.get(offset..offset.checked_add(byte_len)?)?)?
            } else {
                let bytes = data.get(offset..offset.checked_add(count)?)?;
                let mut utf16 = Vec::with_capacity(bytes.len() * 2);
                for byte in bytes {
                    utf16.extend_from_slice(&[*byte, 0]);
                }
                decode_utf16le(&utf16)?
            }
        }
    };
    Some((id, code))
}

fn decode_utf16le(bytes: &[u8]) -> Option<String> {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        units.push(u16::from_le_bytes([pair[0], pair[1]]));
    }
    String::from_utf16(&units).ok()
}

struct BiffRecord<'a> {
    kind: u16,
    data: &'a [u8],
}

struct BiffRecordIter<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> BiffRecordIter<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn next_record(&mut self) -> Result<Option<BiffRecord<'a>>, String> {
        if self.offset == self.bytes.len() {
            return Ok(None);
        }
        let header = self
            .bytes
            .get(self.offset..self.offset.saturating_add(4))
            .ok_or_else(|| "truncated BIFF record header".to_owned())?;
        let kind = u16::from_le_bytes([header[0], header[1]]);
        let len = u16::from_le_bytes([header[2], header[3]]) as usize;
        let data_start = self.offset + 4;
        let data_end = data_start
            .checked_add(len)
            .ok_or_else(|| "BIFF record length overflow".to_owned())?;
        let data = self
            .bytes
            .get(data_start..data_end)
            .ok_or_else(|| format!("truncated BIFF record 0x{kind:04X}"))?;
        self.offset = data_end;
        Ok(Some(BiffRecord { kind, data }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_biff8_format_xf_and_cell_style_indices() {
        let mut global = Vec::new();
        push_record(&mut global, 0x0809, &[0x00, 0x06, 0x05, 0x00]);

        let code = "#,##0.000";
        let mut format = Vec::new();
        format.extend_from_slice(&164_u16.to_le_bytes());
        format.extend_from_slice(&(code.len() as u16).to_le_bytes());
        format.push(0);
        format.extend_from_slice(code.as_bytes());
        push_record(&mut global, 0x041E, &format);
        push_record(&mut global, 0x00E0, &[0, 0, 0, 0]);
        push_record(&mut global, 0x00E0, &[0, 0, 164, 0]);

        let bound_sheet_start = global.len();
        push_record(&mut global, 0x0085, &[0; 8]);
        push_record(&mut global, 0x000A, &[]);

        let sheet_offset = global.len() as u32;
        global[bound_sheet_start + 4..bound_sheet_start + 8]
            .copy_from_slice(&sheet_offset.to_le_bytes());
        push_record(&mut global, 0x0809, &[0x00, 0x06, 0x10, 0x00]);

        let mut number = vec![2, 0, 3, 0, 1, 0];
        number.extend_from_slice(&42.5_f64.to_le_bytes());
        push_record(&mut global, 0x0203, &number);

        let mut mul_rk = vec![5, 0, 1, 0];
        mul_rk.extend_from_slice(&[1, 0, 0, 0, 0, 0]);
        mul_rk.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        mul_rk.extend_from_slice(&2_u16.to_le_bytes());
        push_record(&mut global, 0x00BD, &mul_rk);
        push_record(&mut global, 0x000A, &[]);

        let supplement = parse_workbook_stream(&global).unwrap();
        assert_eq!(supplement.cell(0, (2, 3)), Some(code));
        assert_eq!(supplement.cell(0, (5, 1)), Some(code));
        assert_eq!(supplement.cell(0, (5, 2)), None);
    }

    #[test]
    fn discovers_biff_formula_cells_with_empty_string_caches() {
        let mut global = Vec::new();
        push_record(&mut global, 0x0809, &[0x00, 0x06, 0x05, 0x00]);
        let bound_sheet_start = global.len();
        push_record(&mut global, 0x0085, &[0; 8]);
        push_record(&mut global, 0x000A, &[]);

        let sheet_offset = global.len() as u32;
        global[bound_sheet_start + 4..bound_sheet_start + 8]
            .copy_from_slice(&sheet_offset.to_le_bytes());
        push_record(&mut global, 0x0809, &[0x00, 0x06, 0x10, 0x00]);

        let mut deferred = vec![2, 0, 3, 0, 0, 0];
        deferred.extend_from_slice(&[0x00, 0, 0, 0, 0, 0, 0xFF, 0xFF]);
        deferred.extend_from_slice(&[0; 6]);
        push_record(&mut global, 0x0006, &deferred);
        push_record(&mut global, 0x0207, &[0, 0, 0]);

        let mut immediate = vec![4, 0, 5, 0, 0, 0];
        immediate.extend_from_slice(&[0x03, 0, 0, 0, 0, 0, 0xFF, 0xFF]);
        immediate.extend_from_slice(&[0; 6]);
        push_record(&mut global, 0x0006, &immediate);
        push_record(&mut global, 0x000A, &[]);

        let supplement = parse_workbook_stream(&global).unwrap();
        let cells = supplement
            .empty_formula_cells(0)
            .copied()
            .collect::<HashSet<_>>();
        assert_eq!(cells, HashSet::from([(2, 3), (4, 5)]));
    }

    #[test]
    fn shared_formula_records_between_formula_and_string_keep_the_pending_cache() {
        let mut global = Vec::new();
        push_record(&mut global, 0x0809, &[0x00, 0x06, 0x05, 0x00]);
        let bound_sheet_start = global.len();
        push_record(&mut global, 0x0085, &[0; 8]);
        push_record(&mut global, 0x000A, &[]);

        let sheet_offset = global.len() as u32;
        global[bound_sheet_start + 4..bound_sheet_start + 8]
            .copy_from_slice(&sheet_offset.to_le_bytes());
        push_record(&mut global, 0x0809, &[0x00, 0x06, 0x10, 0x00]);

        // FORMULA with the string sentinel, then an interposed SHRFMLA
        // (MS-XLS allows SHRFMLA/ARRAY/TABLE before the STRING record),
        // then the empty STRING cache.
        let mut deferred = vec![7, 0, 2, 0, 0, 0];
        deferred.extend_from_slice(&[0x00, 0, 0, 0, 0, 0, 0xFF, 0xFF]);
        deferred.extend_from_slice(&[0; 6]);
        push_record(&mut global, 0x0006, &deferred);
        push_record(&mut global, 0x04BC, &[0; 10]);
        push_record(&mut global, 0x0207, &[0, 0, 0]);
        push_record(&mut global, 0x000A, &[]);

        let supplement = parse_workbook_stream(&global).unwrap();
        let cells = supplement
            .empty_formula_cells(0)
            .copied()
            .collect::<HashSet<_>>();
        assert_eq!(cells, HashSet::from([(7, 2)]));
    }

    #[test]
    fn parses_high_byte_biff8_format_strings() {
        let mut data = Vec::new();
        data.extend_from_slice(&164_u16.to_le_bytes());
        data.extend_from_slice(&2_u16.to_le_bytes());
        data.push(1);
        data.extend_from_slice(&('€' as u16).to_le_bytes());
        data.extend_from_slice(&('0' as u16).to_le_bytes());
        assert_eq!(
            parse_format(&data, BiffVersion::Biff8, WINDOWS_1252),
            Some((164, "€0".to_owned()))
        );
    }

    #[test]
    fn biff_panes_report_counts_only_when_window_is_frozen() {
        let mut frozen = Vec::new();
        push_record(&mut frozen, 0x023E, &0x0008_u16.to_le_bytes());
        push_record(&mut frozen, 0x0041, &[2, 0, 3, 0, 0, 0, 0, 0, 0, 0]);
        push_record(&mut frozen, 0x000A, &[]);
        let (_, _, _, view) = parse_sheet_styles(&frozen, &[]).unwrap();
        assert_eq!(
            view,
            SheetView {
                frozen_rows: 3,
                frozen_cols: 2,
            }
        );

        let mut split = Vec::new();
        push_record(&mut split, 0x023E, &0_u16.to_le_bytes());
        push_record(&mut split, 0x0041, &[0xB0, 4, 0x84, 3, 0, 0, 0, 0, 0, 0]);
        push_record(&mut split, 0x000A, &[]);
        let (_, _, _, view) = parse_sheet_styles(&split, &[]).unwrap();
        assert_eq!(view, SheetView::default());
    }

    fn push_record(target: &mut Vec<u8>, kind: u16, data: &[u8]) {
        target.extend_from_slice(&kind.to_le_bytes());
        target.extend_from_slice(&(data.len() as u16).to_le_bytes());
        target.extend_from_slice(data);
    }
}

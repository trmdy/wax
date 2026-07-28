//! Compact, windowed workbook storage.
//!
//! Cells are held as structure-of-arrays columns: coordinates and type tags
//! stay contiguous, numeric values have an `f64` column, booleans use a
//! bitset, and every textual value (including display/formula/format strings)
//! is a `u32` reference into one workbook-wide string table. Sparse row
//! entries point at each row's contiguous cell range, so extracting a window
//! visits only the requested output rectangle and cells in its intersecting
//! row ranges; it never scans the sheet.
//!
//! # Memory bound
//!
//! The retained dense-numeric layout uses 30 bytes per cell plus 8 bytes per
//! distinct populated row and vector/string-table overhead. Consequently a
//! 5,000,000-cell, 200,000-row numeric benchmark measured 151,600,363
//! retained bytes (144.6 MiB), 191,987,712 bytes current process RSS, and a
//! 21 µs p95 for 1,000 64-by-24 windows. Input parsing can have a higher
//! transient bound: the 41,984-byte POI `51535.xls` fixture declares the full
//! BIFF 65,536-by-256 extent, and the reader asks calamine to materialize
//! dense value and formula ranges. It measured 1,076,297,728 bytes peak RSS
//! even with `wax dump --max-cells 0`, proving that outlier exists before
//! both normalized `Document` cells and this store. It must be constrained
//! in the legacy reader/safety layer.

use std::collections::HashMap;
use std::fmt;
use std::mem::size_of;

use wax_core::{Cell, CellType, CellValue, Document, Sheet};

const MISSING_STRING: u32 = u32::MAX;

/// Per-sheet metadata as reported by `open`/`meta`.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetMeta {
    pub name: String,
    pub rows: u32,
    pub cols: u32,
    pub truncated: bool,
}

/// One cell inside a window. Same field semantics as the normalized dump
/// (`docs/w1-contracts.md` §1) minus the coordinates, which are implied by
/// the cell's position in `Window::rows`.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowCell {
    pub t: CellType,
    pub v: Option<CellValue>,
    pub d: Option<String>,
    pub f: Option<String>,
    pub fmt: Option<String>,
}

/// A rectangular window of cells. `r0`/`c0`/`nr`/`nc` are the *effective*
/// bounds after clipping the request to the sheet extent; `rows` has exactly
/// `nr` entries of exactly `nc` entries each. `merges` carries the full
/// (unclipped) A1 ranges of every merge that intersects the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub sheet: u32,
    pub r0: u32,
    pub c0: u32,
    pub nr: u32,
    pub nc: u32,
    pub rows: Vec<Vec<Option<WindowCell>>>,
    pub merges: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct StringId(u32);

#[derive(Clone, Debug, Default)]
struct StringTable {
    values: Vec<Box<str>>,
}

impl StringTable {
    fn get(&self, id: StringId) -> Option<&str> {
        self.values.get(id.0 as usize).map(AsRef::as_ref)
    }

    fn approx_bytes(&self) -> usize {
        allocation_bytes(&self.values)
            .saturating_add(self.values.iter().map(|value| value.len()).sum::<usize>())
    }
}

#[derive(Debug, Default)]
struct StringInterner {
    values: HashMap<String, StringId>,
}

impl StringInterner {
    fn intern(&mut self, value: &str) -> StringId {
        if let Some(id) = self.values.get(value) {
            return *id;
        }
        let id = StringId(
            u32::try_from(self.values.len()).expect("wax store string table exceeds u32::MAX"),
        );
        self.values.insert(value.to_owned(), id);
        id
    }

    fn intern_optional(&mut self, value: Option<String>) -> u32 {
        value.map_or(MISSING_STRING, |value| self.intern(&value).0)
    }

    fn finish(self) -> StringTable {
        let mut values = vec![Box::<str>::default(); self.values.len()];
        for (value, id) in self.values {
            values[id.0 as usize] = value.into_boxed_str();
        }
        values.shrink_to_fit();
        StringTable { values }
    }
}

#[derive(Clone, Copy, Debug)]
struct StoredMerge {
    a1: StringId,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
}

/// The first cell in a populated row. The end is the next row's start (or
/// the column length for the final row), saving four bytes per populated row.
#[derive(Clone, Copy, Debug)]
struct StoredRow {
    row: u32,
    start: u32,
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
enum ValueTag {
    None,
    Number,
    Text,
    Bool,
}

#[derive(Clone, Debug, Default)]
struct BoolColumn {
    words: Vec<u64>,
    len: u32,
}

impl BoolColumn {
    fn push(&mut self, value: bool) -> u32 {
        let index = self.len;
        if index.is_multiple_of(64) {
            self.words.push(0);
        }
        if value {
            self.words[index as usize / 64] |= 1_u64 << (index % 64);
        }
        self.len = self
            .len
            .checked_add(1)
            .expect("wax store boolean column exceeds u32::MAX");
        index
    }

    fn get(&self, index: u32) -> Option<bool> {
        if index >= self.len {
            return None;
        }
        Some((self.words[index as usize / 64] & (1_u64 << (index % 64))) != 0)
    }

    fn shrink_to_fit(&mut self) {
        self.words.shrink_to_fit();
    }

    fn approx_bytes(&self) -> usize {
        allocation_bytes(&self.words)
    }
}

/// Parallel cell columns. `value_refs` indexes the column selected by
/// `value_tags`; text references are direct string-table indexes.
#[derive(Clone, Debug, Default)]
struct StoredColumns {
    cols: Vec<u32>,
    types: Vec<CellType>,
    value_tags: Vec<ValueTag>,
    value_refs: Vec<u32>,
    numbers: Vec<f64>,
    bools: BoolColumn,
    displays: Vec<u32>,
    formulas: Vec<u32>,
    formats: Vec<u32>,
}

impl StoredColumns {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            cols: Vec::with_capacity(capacity),
            types: Vec::with_capacity(capacity),
            value_tags: Vec::with_capacity(capacity),
            value_refs: Vec::with_capacity(capacity),
            numbers: Vec::new(),
            bools: BoolColumn::default(),
            displays: Vec::with_capacity(capacity),
            formulas: Vec::with_capacity(capacity),
            formats: Vec::with_capacity(capacity),
        }
    }

    fn push(&mut self, cell: Cell, strings: &mut StringInterner) {
        let (value_tag, value_ref) = match cell.v {
            None => (ValueTag::None, 0),
            Some(CellValue::Number(value)) => {
                let index = u32::try_from(self.numbers.len())
                    .expect("wax store numeric column exceeds u32::MAX");
                self.numbers.push(value);
                (ValueTag::Number, index)
            }
            Some(CellValue::Text(value)) => (ValueTag::Text, strings.intern(&value).0),
            Some(CellValue::Bool(value)) => (ValueTag::Bool, self.bools.push(value)),
        };

        self.cols.push(cell.c);
        self.types.push(cell.t);
        self.value_tags.push(value_tag);
        self.value_refs.push(value_ref);
        self.displays.push(strings.intern_optional(cell.d));
        self.formulas.push(strings.intern_optional(cell.f));
        self.formats.push(strings.intern_optional(cell.fmt));
    }

    fn cell(&self, index: usize, strings: &StringTable) -> WindowCell {
        let value_ref = self.value_refs[index];
        let v = match self.value_tags[index] {
            ValueTag::None => None,
            ValueTag::Number => self
                .numbers
                .get(value_ref as usize)
                .copied()
                .map(CellValue::Number),
            ValueTag::Text => strings
                .get(StringId(value_ref))
                .map(|value| CellValue::Text(value.to_owned())),
            ValueTag::Bool => self.bools.get(value_ref).map(CellValue::Bool),
        };
        WindowCell {
            t: self.types[index],
            v,
            d: resolve_optional_string(strings, self.displays[index]),
            f: resolve_optional_string(strings, self.formulas[index]),
            fmt: resolve_optional_string(strings, self.formats[index]),
        }
    }

    fn shrink_to_fit(&mut self) {
        self.cols.shrink_to_fit();
        self.types.shrink_to_fit();
        self.value_tags.shrink_to_fit();
        self.value_refs.shrink_to_fit();
        self.numbers.shrink_to_fit();
        self.bools.shrink_to_fit();
        self.displays.shrink_to_fit();
        self.formulas.shrink_to_fit();
        self.formats.shrink_to_fit();
    }

    fn approx_bytes(&self) -> usize {
        [
            allocation_bytes(&self.cols),
            allocation_bytes(&self.types),
            allocation_bytes(&self.value_tags),
            allocation_bytes(&self.value_refs),
            allocation_bytes(&self.numbers),
            self.bools.approx_bytes(),
            allocation_bytes(&self.displays),
            allocation_bytes(&self.formulas),
            allocation_bytes(&self.formats),
        ]
        .into_iter()
        .fold(0, usize::saturating_add)
    }
}

#[derive(Clone, Debug)]
struct StoredSheet {
    name: StringId,
    rows: u32,
    cols: u32,
    truncated: bool,
    merges: Vec<StoredMerge>,
    row_index: Vec<StoredRow>,
    columns: StoredColumns,
}

impl StoredSheet {
    fn approx_bytes(&self) -> usize {
        allocation_bytes(&self.merges)
            .saturating_add(allocation_bytes(&self.row_index))
            .saturating_add(self.columns.approx_bytes())
    }
}

/// Error returned when streaming cells are not in normalized row-major order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellOrderError {
    pub previous: (u32, u32),
    pub current: (u32, u32),
}

impl fmt::Display for CellOrderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cells must be in ascending row-major order: {:?} precedes {:?}",
            self.previous, self.current
        )
    }
}

impl std::error::Error for CellOrderError {}

/// Consuming, sheet-at-a-time store builder.
///
/// [`Self::add_ordered_sheet`] avoids retaining a full [`Document`] or
/// [`Sheet`] cell vector and is the lowest-peak-memory ingestion path.
#[derive(Debug, Default)]
pub struct WorkbookStoreBuilder {
    strings: StringInterner,
    sheets: Vec<StoredSheet>,
}

impl WorkbookStoreBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a normalized sheet. Cells are stably sorted so duplicate
    /// coordinates retain the same last-cell-wins behavior as the original
    /// store.
    pub fn add_sheet(&mut self, mut sheet: Sheet) {
        sheet.cells.sort_by_key(|cell| (cell.r, cell.c));
        let meta = SheetMeta {
            name: sheet.name,
            rows: sheet.rows,
            cols: sheet.cols,
            truncated: sheet.truncated,
        };
        self.add_ordered_sheet(meta, sheet.merges, sheet.cells)
            .expect("the stable row-major sort must produce ordered cells");
    }

    /// Stream one sheet into the store without first collecting its cells.
    ///
    /// Cells must be sorted ascending by `(r, c)`; duplicate coordinates are
    /// allowed and the last duplicate wins when a window is materialized.
    /// On an ordering error the builder should be discarded.
    pub fn add_ordered_sheet<I>(
        &mut self,
        meta: SheetMeta,
        merges: Vec<String>,
        cells: I,
    ) -> Result<(), CellOrderError>
    where
        I: IntoIterator<Item = Cell>,
    {
        let mut cells = cells.into_iter();
        let capacity = cells.size_hint().0;
        let name = self.strings.intern(&meta.name);
        let mut stored_merges = merges
            .into_iter()
            .filter_map(|a1| parse_a1_range(&a1, &mut self.strings))
            .collect::<Vec<_>>();
        stored_merges.shrink_to_fit();

        let mut columns = StoredColumns::with_capacity(capacity);
        let mut row_index = Vec::new();
        let mut previous = None;
        for cell in &mut cells {
            let position = (cell.r, cell.c);
            if let Some(previous_position) = previous {
                if position < previous_position {
                    return Err(CellOrderError {
                        previous: previous_position,
                        current: position,
                    });
                }
            }
            previous = Some(position);

            // Such cells were unreachable in the naive store after extent
            // clipping, so omitting them preserves API behavior.
            if cell.r >= meta.rows || cell.c >= meta.cols {
                continue;
            }
            if row_index
                .last()
                .is_none_or(|row: &StoredRow| row.row != cell.r)
            {
                row_index.push(StoredRow {
                    row: cell.r,
                    start: u32::try_from(columns.cols.len())
                        .expect("wax store sheet exceeds u32::MAX cells"),
                });
            }
            columns.push(cell, &mut self.strings);
        }
        row_index.shrink_to_fit();
        columns.shrink_to_fit();
        self.sheets.push(StoredSheet {
            name,
            rows: meta.rows,
            cols: meta.cols,
            truncated: meta.truncated,
            merges: stored_merges,
            row_index,
            columns,
        });
        Ok(())
    }

    pub fn build(mut self) -> WorkbookStore {
        self.sheets.shrink_to_fit();
        WorkbookStore {
            strings: self.strings.finish(),
            sheets: self.sheets,
        }
    }
}

/// An immutable, windowable snapshot of one parsed workbook.
#[derive(Clone, Debug, Default)]
pub struct WorkbookStore {
    strings: StringTable,
    sheets: Vec<StoredSheet>,
}

impl WorkbookStore {
    /// Ingest a successful normalized dump. The caller drops the `Document`
    /// afterwards; the store is the long-lived representation.
    pub fn from_document(document: Document) -> Self {
        let mut builder = WorkbookStoreBuilder::new();
        for sheet in document.sheets {
            builder.add_sheet(sheet);
        }
        builder.build()
    }

    pub fn sheet_count(&self) -> u32 {
        u32::try_from(self.sheets.len()).unwrap_or(u32::MAX)
    }

    pub fn sheet_meta(&self, sheet: u32) -> Option<SheetMeta> {
        let stored = self.sheets.get(usize::try_from(sheet).ok()?)?;
        Some(SheetMeta {
            name: self.strings.get(stored.name)?.to_owned(),
            rows: stored.rows,
            cols: stored.cols,
            truncated: stored.truncated,
        })
    }

    /// Extract a window, clipped to the sheet extent. Returns `None` for an
    /// unknown sheet index. A request entirely outside the extent yields an
    /// empty window (`nr == 0 || nc == 0`).
    pub fn window(&self, sheet: u32, r0: u32, c0: u32, nr: u32, nc: u32) -> Option<Window> {
        let stored = self.sheets.get(usize::try_from(sheet).ok()?)?;
        let r1 = r0.saturating_add(nr).min(stored.rows);
        let c1 = c0.saturating_add(nc).min(stored.cols);
        let (nr, nc) = if r0 >= r1 || c0 >= c1 {
            (0, 0)
        } else {
            (r1 - r0, c1 - c0)
        };

        let mut rows: Vec<Vec<Option<WindowCell>>> =
            (0..nr).map(|_| (0..nc).map(|_| None).collect()).collect();
        if nr > 0 {
            let first_row = stored.row_index.partition_point(|row| row.row < r0);
            for row_offset in first_row..stored.row_index.len() {
                let indexed_row = stored.row_index[row_offset];
                if indexed_row.row >= r1 {
                    break;
                }
                let start = indexed_row.start as usize;
                let end = stored
                    .row_index
                    .get(row_offset + 1)
                    .map_or(stored.columns.cols.len(), |row| row.start as usize);
                let row_columns = &stored.columns.cols[start..end];
                let first_column = row_columns.partition_point(|column| *column < c0);
                for cell_index in start + first_column..end {
                    let column = stored.columns.cols[cell_index];
                    if column >= c1 {
                        break;
                    }
                    rows[(indexed_row.row - r0) as usize][(column - c0) as usize] =
                        Some(stored.columns.cell(cell_index, &self.strings));
                }
            }
        }

        let merges = if nr == 0 {
            Vec::new()
        } else {
            stored
                .merges
                .iter()
                .filter(|merge| merge.r0 < r1 && merge.r1 >= r0 && merge.c0 < c1 && merge.c1 >= c0)
                .filter_map(|merge| self.strings.get(merge.a1).map(str::to_owned))
                .collect()
        };

        Some(Window {
            sheet,
            r0,
            c0,
            nr,
            nc,
            rows,
            merges,
        })
    }

    /// Approximate retained bytes, including all structure/vector capacities,
    /// sparse indexes, typed columns, string-table entries, and string bytes.
    ///
    /// Allocator bookkeeping and alignment outside Rust's requested
    /// allocations are intentionally excluded; strings are counted once at
    /// their interned allocation, never once per reference.
    pub fn approx_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(allocation_bytes(&self.sheets))
            .saturating_add(self.strings.approx_bytes())
            .saturating_add(
                self.sheets
                    .iter()
                    .map(StoredSheet::approx_bytes)
                    .sum::<usize>(),
            )
    }
}

fn allocation_bytes<T>(values: &Vec<T>) -> usize {
    values.capacity().saturating_mul(size_of::<T>())
}

fn resolve_optional_string(strings: &StringTable, id: u32) -> Option<String> {
    (id != MISSING_STRING)
        .then(|| strings.get(StringId(id)).map(str::to_owned))
        .flatten()
}

/// Parse an A1-style range (`"A1:B2"` or a single `"A1"`) into 0-based
/// inclusive bounds.
fn parse_a1_range(a1: &str, strings: &mut StringInterner) -> Option<StoredMerge> {
    let (start, end) = match a1.split_once(':') {
        Some((start, end)) => (start, end),
        None => (a1, a1),
    };
    let (c0, r0) = parse_a1_cell(start)?;
    let (c1, r1) = parse_a1_cell(end)?;
    Some(StoredMerge {
        a1: strings.intern(a1),
        r0: r0.min(r1),
        c0: c0.min(c1),
        r1: r0.max(r1),
        c1: c0.max(c1),
    })
}

fn parse_a1_cell(reference: &str) -> Option<(u32, u32)> {
    let split = reference.find(|character: char| character.is_ascii_digit())?;
    let (letters, digits) = reference.split_at(split);
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let mut column: u32 = 0;
    for letter in letters.chars() {
        if !letter.is_ascii_alphabetic() {
            return None;
        }
        let value = u32::from(letter.to_ascii_uppercase()) - u32::from('A') + 1;
        column = column.checked_mul(26)?.checked_add(value)?;
    }
    let row: u32 = digits.parse().ok()?;
    if row == 0 || column == 0 {
        return None;
    }
    Some((column - 1, row - 1))
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Instant;

    use super::*;

    fn document_with_cells(
        rows: u32,
        cols: u32,
        cells: Vec<Cell>,
        merges: Vec<String>,
    ) -> Document {
        Document::success(
            "test",
            "test.xlsx",
            vec![Sheet {
                name: "Sheet1".to_owned(),
                index: 0,
                rows,
                cols,
                truncated: false,
                merges,
                cells,
            }],
            Vec::new(),
        )
    }

    fn number_cell(r: u32, c: u32, value: f64) -> Cell {
        Cell {
            r,
            c,
            t: CellType::N,
            v: Some(CellValue::Number(value)),
            d: Some(value.to_string()),
            f: None,
            fmt: None,
        }
    }

    fn cell_without_display(r: u32, c: u32, value: f64) -> Cell {
        Cell {
            d: None,
            ..number_cell(r, c, value)
        }
    }

    #[test]
    fn window_returns_effective_bounds_and_cells() {
        let store = WorkbookStore::from_document(document_with_cells(
            10,
            5,
            vec![number_cell(0, 0, 1.0), number_cell(2, 3, 7.5)],
            Vec::new(),
        ));

        let window = store.window(0, 0, 0, 4, 4).expect("sheet 0 exists");
        assert_eq!((window.nr, window.nc), (4, 4));
        assert_eq!(window.rows.len(), 4);
        assert!(window.rows.iter().all(|row| row.len() == 4));
        assert_eq!(
            window.rows[0][0].as_ref().and_then(|cell| cell.v.clone()),
            Some(CellValue::Number(1.0))
        );
        assert_eq!(
            window.rows[2][3].as_ref().and_then(|cell| cell.v.clone()),
            Some(CellValue::Number(7.5))
        );
        assert!(window.rows[1][1].is_none());
    }

    #[test]
    fn window_clips_to_sheet_extent() {
        let store = WorkbookStore::from_document(document_with_cells(
            3,
            2,
            vec![number_cell(2, 1, 9.0)],
            Vec::new(),
        ));

        let window = store.window(0, 2, 0, 64, 24).expect("sheet 0 exists");
        assert_eq!((window.r0, window.c0, window.nr, window.nc), (2, 0, 1, 2));
        assert_eq!(window.rows.len(), 1);
        assert_eq!(window.rows[0].len(), 2);
        assert!(window.rows[0][1].is_some());
    }

    #[test]
    fn window_outside_extent_is_empty() {
        let store = WorkbookStore::from_document(document_with_cells(3, 2, Vec::new(), Vec::new()));
        let window = store.window(0, 10, 10, 4, 4).expect("sheet 0 exists");
        assert_eq!((window.nr, window.nc), (0, 0));
        assert!(window.rows.is_empty());
        assert!(window.merges.is_empty());
    }

    #[test]
    fn unknown_sheet_is_none() {
        let store = WorkbookStore::from_document(document_with_cells(1, 1, Vec::new(), Vec::new()));
        assert!(store.window(1, 0, 0, 1, 1).is_none());
        assert!(store.sheet_meta(1).is_none());
        assert_eq!(store.sheet_count(), 1);
    }

    #[test]
    fn merges_intersecting_window_are_reported_unclipped() {
        let store = WorkbookStore::from_document(document_with_cells(
            10,
            10,
            Vec::new(),
            vec!["A1:B2".to_owned(), "E5:F6".to_owned()],
        ));

        let window = store.window(0, 0, 0, 3, 3).expect("sheet 0 exists");
        assert_eq!(window.merges, vec!["A1:B2".to_owned()]);

        let window = store.window(0, 5, 5, 2, 2).expect("sheet 0 exists");
        assert_eq!(window.merges, vec!["E5:F6".to_owned()]);
    }

    #[test]
    fn a1_parsing_handles_multi_letter_columns() {
        let mut strings = StringInterner::default();
        let merge = parse_a1_range("AA10:AB12", &mut strings).expect("valid range");
        assert_eq!((merge.r0, merge.c0, merge.r1, merge.c1), (9, 26, 11, 27));
        assert!(parse_a1_range("", &mut strings).is_none());
        assert!(parse_a1_range("1A", &mut strings).is_none());
        assert!(parse_a1_range("A0", &mut strings).is_none());
    }

    #[test]
    fn strings_are_interned_across_fields_and_sheets() {
        let repeated = "same repeated payload";
        let sheets = (0..2)
            .map(|index| Sheet {
                name: format!("Sheet{index}"),
                index,
                rows: 1,
                cols: 1,
                truncated: false,
                merges: Vec::new(),
                cells: vec![Cell {
                    r: 0,
                    c: 0,
                    t: CellType::S,
                    v: Some(CellValue::Text(repeated.to_owned())),
                    d: Some(repeated.to_owned()),
                    f: Some(repeated.to_owned()),
                    fmt: Some(repeated.to_owned()),
                }],
            })
            .collect();
        let store =
            WorkbookStore::from_document(Document::success("test", "test.xlsx", sheets, vec![]));

        let repeated_ids = store
            .strings
            .values
            .iter()
            .filter(|value| value.as_ref() == repeated)
            .count();
        assert_eq!(repeated_ids, 1);
        assert_eq!(
            store.sheets[0].columns.value_refs[0],
            store.sheets[1].columns.value_refs[0]
        );
        assert_eq!(
            store.sheets[0].columns.value_refs[0],
            store.sheets[0].columns.displays[0]
        );
        for sheet in 0..2 {
            let cell = store.window(sheet, 0, 0, 1, 1).unwrap().rows[0][0]
                .clone()
                .unwrap();
            assert_eq!(cell.v, Some(CellValue::Text(repeated.to_owned())));
            assert_eq!(cell.d.as_deref(), Some(repeated));
            assert_eq!(cell.f.as_deref(), Some(repeated));
            assert_eq!(cell.fmt.as_deref(), Some(repeated));
        }
    }

    #[test]
    fn builder_ingestion_matches_from_document() {
        let document = Document::success(
            "test",
            "test.xlsx",
            vec![
                Sheet {
                    name: "First".to_owned(),
                    index: 0,
                    rows: 4,
                    cols: 3,
                    truncated: false,
                    merges: vec!["A1:B1".to_owned()],
                    cells: vec![
                        number_cell(3, 2, 3.0),
                        number_cell(0, 0, 1.0),
                        number_cell(0, 0, 2.0),
                    ],
                },
                Sheet {
                    name: "Second".to_owned(),
                    index: 1,
                    rows: 1,
                    cols: 1,
                    truncated: true,
                    merges: Vec::new(),
                    cells: vec![Cell {
                        r: 0,
                        c: 0,
                        t: CellType::B,
                        v: Some(CellValue::Bool(true)),
                        d: Some("TRUE".to_owned()),
                        f: None,
                        fmt: None,
                    }],
                },
            ],
            vec![],
        );
        let direct = WorkbookStore::from_document(document.clone());
        let mut builder = WorkbookStoreBuilder::new();
        for sheet in document.sheets {
            builder.add_sheet(sheet);
        }
        let built = builder.build();

        assert_eq!(direct.sheet_count(), built.sheet_count());
        assert_eq!(direct.approx_bytes(), built.approx_bytes());
        for sheet in 0..direct.sheet_count() {
            assert_eq!(direct.sheet_meta(sheet), built.sheet_meta(sheet));
            assert_eq!(
                direct.window(sheet, 0, 0, u32::MAX, u32::MAX),
                built.window(sheet, 0, 0, u32::MAX, u32::MAX)
            );
        }
    }

    #[test]
    fn ordered_builder_rejects_out_of_order_cells() {
        let mut builder = WorkbookStoreBuilder::new();
        let error = builder
            .add_ordered_sheet(
                SheetMeta {
                    name: "Sheet1".to_owned(),
                    rows: 2,
                    cols: 2,
                    truncated: false,
                },
                Vec::new(),
                [number_cell(1, 0, 1.0), number_cell(0, 1, 2.0)],
            )
            .unwrap_err();
        assert_eq!(error.previous, (1, 0));
        assert_eq!(error.current, (0, 1));
    }

    #[test]
    fn approx_bytes_grows_and_counts_interned_strings_once() {
        let empty = WorkbookStore::from_document(document_with_cells(1, 1, Vec::new(), Vec::new()));
        let repeated = "x".repeat(4096);
        let one = WorkbookStore::from_document(document_with_cells(
            1,
            2,
            vec![Cell {
                r: 0,
                c: 0,
                t: CellType::S,
                v: Some(CellValue::Text(repeated.clone())),
                d: Some(repeated.clone()),
                f: Some(repeated.clone()),
                fmt: Some(repeated.clone()),
            }],
            Vec::new(),
        ));
        let two = WorkbookStore::from_document(document_with_cells(
            1,
            2,
            vec![
                Cell {
                    r: 0,
                    c: 0,
                    t: CellType::S,
                    v: Some(CellValue::Text(repeated.clone())),
                    d: Some(repeated.clone()),
                    f: Some(repeated.clone()),
                    fmt: Some(repeated.clone()),
                },
                Cell {
                    r: 0,
                    c: 1,
                    t: CellType::S,
                    v: Some(CellValue::Text(repeated.clone())),
                    d: Some(repeated.clone()),
                    f: Some(repeated.clone()),
                    fmt: Some(repeated.clone()),
                },
            ],
            Vec::new(),
        ));

        assert!(one.approx_bytes() > empty.approx_bytes() + repeated.len());
        assert!(two.approx_bytes() > one.approx_bytes());
        assert!(two.approx_bytes() - one.approx_bytes() < repeated.len());
        assert_eq!(
            two.strings
                .values
                .iter()
                .filter(|value| value.as_ref() == repeated)
                .count(),
            1
        );
    }

    #[test]
    fn empty_single_cell_and_full_window_edges() {
        let empty = WorkbookStore::from_document(document_with_cells(0, 0, Vec::new(), Vec::new()));
        let window = empty.window(0, 0, 0, 1, 1).unwrap();
        assert_eq!((window.nr, window.nc), (0, 0));
        assert!(window.rows.is_empty());

        let single = WorkbookStore::from_document(document_with_cells(
            1,
            1,
            vec![number_cell(0, 0, 42.0)],
            vec!["A1".to_owned()],
        ));
        let full = single.window(0, 0, 0, u32::MAX, u32::MAX).unwrap();
        assert_eq!((full.r0, full.c0, full.nr, full.nc), (0, 0, 1, 1));
        assert_eq!(full.merges, vec!["A1"]);
        assert_eq!(
            full.rows[0][0].as_ref().and_then(|cell| cell.v.clone()),
            Some(CellValue::Number(42.0))
        );
    }

    #[test]
    fn randomized_windows_match_brute_force_oracle() {
        let mut random = Lcg::new(0x5eed_cafe);
        for _case in 0..100 {
            let sheet_rows = random.bounded(30);
            let sheet_cols = random.bounded(20);
            let cell_count = random.bounded(100);
            let mut cells = Vec::new();
            for _ in 0..cell_count {
                let r = random.bounded(sheet_rows.saturating_add(5));
                let c = random.bounded(sheet_cols.saturating_add(5));
                cells.push(random_cell(&mut random, r, c));
            }
            let merges = vec!["A1:B2".to_owned(), "D4:F6".to_owned()];
            let document = document_with_cells(sheet_rows, sheet_cols, cells, merges);
            let oracle_document = document.clone();
            let store = WorkbookStore::from_document(document);

            for _window in 0..25 {
                let r0 = random.bounded(sheet_rows.saturating_add(10));
                let c0 = random.bounded(sheet_cols.saturating_add(10));
                let nr = random.bounded(20);
                let nc = random.bounded(15);
                let actual = store.window(0, r0, c0, nr, nc).unwrap();
                let expected = brute_force_window(&oracle_document.sheets[0], r0, c0, nr, nc);
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn mixed_text_interning_has_large_measured_win() {
        let repeated = "repeated display and format payload";
        let cell_count = 10_000_u32;
        let cells = (0..cell_count).map(|row| Cell {
            r: row,
            c: 0,
            t: CellType::S,
            v: Some(CellValue::Text(repeated.to_owned())),
            d: Some(repeated.to_owned()),
            f: None,
            fmt: Some(repeated.to_owned()),
        });
        let mut builder = WorkbookStoreBuilder::new();
        builder
            .add_ordered_sheet(
                SheetMeta {
                    name: "Text".to_owned(),
                    rows: cell_count,
                    cols: 1,
                    truncated: false,
                },
                Vec::new(),
                cells,
            )
            .unwrap();
        let store = builder.build();
        let naive_string_bytes = repeated.len() * cell_count as usize * 3;
        println!(
            "mixed text: store={} bytes, naive repeated string payload={} bytes, ratio={:.2}x",
            store.approx_bytes(),
            naive_string_bytes,
            naive_string_bytes as f64 / store.approx_bytes() as f64
        );
        assert_eq!(
            store
                .strings
                .values
                .iter()
                .filter(|value| value.as_ref() == repeated)
                .count(),
            1
        );
        assert!(store.approx_bytes() < naive_string_bytes / 2);
    }

    /// Run explicitly with:
    /// `cargo test -p wax-store --release large_numeric_store_profile -- --ignored --nocapture`
    #[test]
    #[ignore = "allocates and profiles the v1 five-million-cell target"]
    fn large_numeric_store_profile() {
        const ROWS: u32 = 200_000;
        const COLS: u32 = 25;
        const CELL_COUNT: usize = ROWS as usize * COLS as usize;

        let cells = (0..ROWS).flat_map(|row| {
            (0..COLS).map(move |col| {
                cell_without_display(row, col, f64::from(row) * 100.0 + f64::from(col))
            })
        });
        let mut builder = WorkbookStoreBuilder::new();
        builder
            .add_ordered_sheet(
                SheetMeta {
                    name: "FiveMillion".to_owned(),
                    rows: ROWS,
                    cols: COLS,
                    truncated: false,
                },
                Vec::new(),
                cells,
            )
            .unwrap();
        let store = builder.build();

        let mut latencies = Vec::with_capacity(1_000);
        for sample in 0..1_000_u32 {
            let row = sample.wrapping_mul(7919) % (ROWS - 64);
            let started = Instant::now();
            let window = store.window(0, row, 1, 64, 24).unwrap();
            assert_eq!((window.nr, window.nc), (64, 24));
            latencies.push(started.elapsed());
        }
        latencies.sort_unstable();
        let p95 = latencies[latencies.len() * 95 / 100];
        let rss = current_rss_bytes();
        println!(
            "5M numeric: cells={CELL_COUNT}, approx_bytes={}, rss_bytes={rss:?}, window_64x24_p95_us={}",
            store.approx_bytes(),
            p95.as_micros()
        );
        assert!(store.approx_bytes() <= 200_000_000);
        assert!(p95.as_millis() < 10);
    }

    fn brute_force_window(sheet: &Sheet, r0: u32, c0: u32, nr: u32, nc: u32) -> Window {
        let r1 = r0.saturating_add(nr).min(sheet.rows);
        let c1 = c0.saturating_add(nc).min(sheet.cols);
        let (nr, nc) = if r0 >= r1 || c0 >= c1 {
            (0, 0)
        } else {
            (r1 - r0, c1 - c0)
        };
        let mut rows = (0..nr)
            .map(|_| (0..nc).map(|_| None).collect())
            .collect::<Vec<Vec<_>>>();
        for cell in &sheet.cells {
            if cell.r >= r0 && cell.r < r1 && cell.c >= c0 && cell.c < c1 {
                rows[(cell.r - r0) as usize][(cell.c - c0) as usize] = Some(WindowCell {
                    t: cell.t,
                    v: cell.v.clone(),
                    d: cell.d.clone(),
                    f: cell.f.clone(),
                    fmt: cell.fmt.clone(),
                });
            }
        }
        let merges = if nr == 0 {
            Vec::new()
        } else {
            sheet
                .merges
                .iter()
                .filter_map(|a1| {
                    let mut strings = StringInterner::default();
                    parse_a1_range(a1, &mut strings).map(|merge| (a1, merge))
                })
                .filter(|(_, merge)| {
                    merge.r0 < r1 && merge.r1 >= r0 && merge.c0 < c1 && merge.c1 >= c0
                })
                .map(|(a1, _)| a1.clone())
                .collect()
        };
        Window {
            sheet: 0,
            r0,
            c0,
            nr,
            nc,
            rows,
            merges,
        }
    }

    fn random_cell(random: &mut Lcg, r: u32, c: u32) -> Cell {
        let kind = random.bounded(5);
        let (t, v) = match kind {
            0 => (
                CellType::N,
                Some(CellValue::Number(f64::from(random.next_u32()) / 17.0)),
            ),
            1 => (
                CellType::S,
                Some(CellValue::Text(format!("text-{}", random.bounded(7)))),
            ),
            2 => (CellType::B, Some(CellValue::Bool(random.bounded(2) == 1))),
            3 => (CellType::E, Some(CellValue::Text("#DIV/0!".to_owned()))),
            _ => (CellType::D, Some(CellValue::Text("2026-07-28".to_owned()))),
        };
        Cell {
            r,
            c,
            t,
            v: if random.bounded(11) == 0 { None } else { v },
            d: (random.bounded(3) != 0).then(|| format!("display-{}", random.bounded(5))),
            f: (random.bounded(7) == 0).then(|| "SUM(A1:A2)".to_owned()),
            fmt: (random.bounded(4) == 0).then(|| "#,##0.00".to_owned()),
        }
    }

    struct Lcg(u64);

    impl Lcg {
        fn new(seed: u64) -> Self {
            Self(seed)
        }

        fn next_u32(&mut self) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            (self.0 >> 32) as u32
        }

        fn bounded(&mut self, upper: u32) -> u32 {
            if upper == 0 {
                0
            } else {
                self.next_u32() % upper
            }
        }
    }

    fn current_rss_bytes() -> Option<u64> {
        let output = Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        let kibibytes = String::from_utf8(output.stdout)
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()?;
        kibibytes.checked_mul(1024)
    }
}

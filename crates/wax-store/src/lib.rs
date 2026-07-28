//! Windowed workbook store.
//!
//! The public API here is frozen by `docs/w3-contracts.md` §2: W3A (the
//! protocol server) codes against it while W3B replaces the internals with a
//! compact columnar representation (typed columns + string table, O(window)
//! `window()`). This naive implementation exists only so the seam compiles
//! and behaves correctly from day one — do not invest in making it fast.

use wax_core::{CellType, CellValue, Document};

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

#[derive(Clone, Debug)]
struct StoredMerge {
    a1: String,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
}

#[derive(Clone, Debug)]
struct StoredSheet {
    meta: SheetMeta,
    merges: Vec<StoredMerge>,
    cells: Vec<StoredCell>,
}

#[derive(Clone, Debug)]
struct StoredCell {
    r: u32,
    c: u32,
    cell: WindowCell,
}

/// An immutable, windowable snapshot of one parsed workbook.
#[derive(Clone, Debug, Default)]
pub struct WorkbookStore {
    sheets: Vec<StoredSheet>,
}

impl WorkbookStore {
    /// Ingest a successful normalized dump. The caller drops the `Document`
    /// afterwards; the store is the long-lived representation.
    pub fn from_document(document: Document) -> Self {
        let sheets = document
            .sheets
            .into_iter()
            .map(|sheet| StoredSheet {
                meta: SheetMeta {
                    name: sheet.name,
                    rows: sheet.rows,
                    cols: sheet.cols,
                    truncated: sheet.truncated,
                },
                merges: sheet
                    .merges
                    .iter()
                    .filter_map(|a1| parse_a1_range(a1))
                    .collect(),
                cells: sheet
                    .cells
                    .into_iter()
                    .map(|cell| StoredCell {
                        r: cell.r,
                        c: cell.c,
                        cell: WindowCell {
                            t: cell.t,
                            v: cell.v,
                            d: cell.d,
                            f: cell.f,
                            fmt: cell.fmt,
                        },
                    })
                    .collect(),
            })
            .collect();
        Self { sheets }
    }

    pub fn sheet_count(&self) -> u32 {
        u32::try_from(self.sheets.len()).unwrap_or(u32::MAX)
    }

    pub fn sheet_meta(&self, sheet: u32) -> Option<SheetMeta> {
        self.sheets
            .get(usize::try_from(sheet).ok()?)
            .map(|stored| stored.meta.clone())
    }

    /// Extract a window, clipped to the sheet extent. Returns `None` for an
    /// unknown sheet index. A request entirely outside the extent yields an
    /// empty window (`nr == 0 || nc == 0`).
    pub fn window(&self, sheet: u32, r0: u32, c0: u32, nr: u32, nc: u32) -> Option<Window> {
        let stored = self.sheets.get(usize::try_from(sheet).ok()?)?;
        let r1 = r0.saturating_add(nr).min(stored.meta.rows);
        let c1 = c0.saturating_add(nc).min(stored.meta.cols);
        let (nr, nc) = if r0 >= r1 || c0 >= c1 {
            (0, 0)
        } else {
            (r1 - r0, c1 - c0)
        };

        let mut rows: Vec<Vec<Option<WindowCell>>> =
            (0..nr).map(|_| (0..nc).map(|_| None).collect()).collect();
        if nr > 0 {
            for stored_cell in &stored.cells {
                if stored_cell.r >= r0
                    && stored_cell.r < r0 + nr
                    && stored_cell.c >= c0
                    && stored_cell.c < c0 + nc
                {
                    rows[(stored_cell.r - r0) as usize][(stored_cell.c - c0) as usize] =
                        Some(stored_cell.cell.clone());
                }
            }
        }

        let merges = if nr == 0 {
            Vec::new()
        } else {
            stored
                .merges
                .iter()
                .filter(|merge| {
                    merge.r0 < r0 + nr && merge.r1 >= r0 && merge.c0 < c0 + nc && merge.c1 >= c0
                })
                .map(|merge| merge.a1.clone())
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

    /// Rough in-memory footprint in bytes. The W3B implementation must make
    /// this an honest measurement backing the documented memory bound.
    pub fn approx_bytes(&self) -> usize {
        let mut total = std::mem::size_of::<Self>();
        for sheet in &self.sheets {
            total += std::mem::size_of::<StoredSheet>() + sheet.meta.name.len();
            for merge in &sheet.merges {
                total += std::mem::size_of::<StoredMerge>() + merge.a1.len();
            }
            for stored in &sheet.cells {
                total += std::mem::size_of::<StoredCell>();
                if let Some(CellValue::Text(text)) = &stored.cell.v {
                    total += text.len();
                }
                for text in [&stored.cell.d, &stored.cell.f, &stored.cell.fmt]
                    .into_iter()
                    .flatten()
                {
                    total += text.len();
                }
            }
        }
        total
    }
}

/// Parse an A1-style range (`"A1:B2"` or a single `"A1"`) into 0-based
/// inclusive bounds.
fn parse_a1_range(a1: &str) -> Option<StoredMerge> {
    let (start, end) = match a1.split_once(':') {
        Some((start, end)) => (start, end),
        None => (a1, a1),
    };
    let (c0, r0) = parse_a1_cell(start)?;
    let (c1, r1) = parse_a1_cell(end)?;
    Some(StoredMerge {
        a1: a1.to_owned(),
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
    use super::*;
    use wax_core::{Cell, Sheet};

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
        let merge = parse_a1_range("AA10:AB12").expect("valid range");
        assert_eq!((merge.r0, merge.c0, merge.r1, merge.c1), (9, 26, 11, 27));
        assert!(parse_a1_range("").is_none());
        assert!(parse_a1_range("1A").is_none());
        assert!(parse_a1_range("A0").is_none());
    }

    #[test]
    fn approx_bytes_grows_with_content() {
        let empty = WorkbookStore::from_document(document_with_cells(1, 1, Vec::new(), Vec::new()));
        let full = WorkbookStore::from_document(document_with_cells(
            1,
            1,
            vec![Cell {
                r: 0,
                c: 0,
                t: CellType::S,
                v: Some(CellValue::Text("hello world".to_owned())),
                d: Some("hello world".to_owned()),
                f: None,
                fmt: None,
            }],
            Vec::new(),
        ));
        assert!(full.approx_bytes() > empty.approx_bytes());
    }
}

use std::path::{Path, PathBuf};

use wax_core::{CellType, CellValue};
use wax_read::{Reader, ReaderOptions, StubReader, STUB_WARNING};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("tiny.xlsx")
}

#[test]
fn reads_sheet_order_extents_numeric_and_inline_cells() {
    let path = fixture_path();
    let document = StubReader.read(&path, ReaderOptions::default());

    assert!(document.ok);
    assert_eq!(document.warnings, [STUB_WARNING]);
    assert_eq!(document.sheets.len(), 2);
    assert_eq!(document.sheets[0].name, "Numbers");
    assert_eq!((document.sheets[0].rows, document.sheets[0].cols), (3, 3));
    assert_eq!(document.sheets[0].cells.len(), 3);
    assert_eq!(document.sheets[0].cells[0].t, CellType::N);
    assert_eq!(document.sheets[0].cells[0].v, Some(CellValue::Number(42.5)));
    assert_eq!(
        document.sheets[0].cells[2].v,
        Some(CellValue::Text("North & South".to_owned()))
    );
    assert_eq!(document.sheets[1].name, "Inline");
    assert_eq!((document.sheets[1].rows, document.sheets[1].cols), (2, 2));
    assert_eq!(document.sheets[1].cells.len(), 1);
}

#[test]
fn cell_cap_is_global_and_marks_affected_sheets_and_document() {
    let path = fixture_path();
    let document = StubReader.read(
        &path,
        ReaderOptions {
            max_cells: 1,
            ..ReaderOptions::default()
        },
    );

    assert!(document.ok);
    assert!(document.truncated);
    assert_eq!(document.sheets[0].cells.len(), 1);
    assert!(document.sheets[0].truncated);
    assert!(document.sheets[1].cells.is_empty());
    assert!(document.sheets[1].truncated);
    assert_eq!((document.sheets[0].rows, document.sheets[0].cols), (3, 3));
}

#[test]
fn unsupported_input_is_a_normalized_failure() {
    let temp = tempfile::tempdir().expect("temporary directory should be created");
    let path = temp.path().join("values.csv");
    std::fs::write(&path, "one,two\n").expect("fixture should be written");

    let document = StubReader.read(&path, ReaderOptions::default());

    assert!(!document.ok);
    assert_eq!(
        document.error.expect("failure should carry an error").code,
        "unsupported"
    );
    assert_eq!(document.warnings, [STUB_WARNING]);
}

#[test]
fn malformed_xlsx_is_reported_as_bad_zip() {
    let temp = tempfile::tempdir().expect("temporary directory should be created");
    let path = temp.path().join("broken.xlsx");
    std::fs::write(&path, "not a zip").expect("fixture should be written");

    let document = StubReader.read(&path, ReaderOptions::default());

    assert!(!document.ok);
    assert_eq!(
        document.error.expect("failure should carry an error").code,
        "bad_zip"
    );
}

#[test]
fn zero_timeout_is_reported_without_parsing() {
    let path = fixture_path();
    let document = StubReader.read(
        &path,
        ReaderOptions {
            timeout_ms: 0,
            ..ReaderOptions::default()
        },
    );

    assert!(!document.ok);
    assert_eq!(
        document.error.expect("failure should carry an error").code,
        "timeout"
    );
}

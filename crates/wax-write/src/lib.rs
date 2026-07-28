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

use std::path::Path;
use std::sync::atomic::AtomicBool;

use wax_store::WorkbookStore;

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
/// W4A implements this via `rust_xlsxwriter`; until that lands it returns a
/// structured `internal` error so protocol wiring and harness code can build
/// and degrade gracefully.
pub fn write_xlsx(
    store: &WorkbookStore,
    out: &Path,
    cancel: &AtomicBool,
) -> Result<ExportOutcome, WriteError> {
    let _ = (store, out, cancel);
    Err(WriteError::new(
        "internal",
        "xlsx export is not implemented yet (wax-write W4A)",
    ))
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
    let _ = (store, sheet, out, cancel);
    Err(WriteError::new(
        "internal",
        "csv export is not implemented in wax-write yet (W4A); serve keeps its own CSV path until then",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wax_core::Document;

    #[test]
    fn stubs_return_structured_internal_errors() {
        let store = WorkbookStore::from_document(Document::success(
            "0.1.0",
            "empty.xlsx",
            Vec::new(),
            Vec::new(),
        ));
        let cancel = AtomicBool::new(false);
        let xlsx = write_xlsx(&store, Path::new("/tmp/out.xlsx"), &cancel).unwrap_err();
        assert_eq!(xlsx.code, "internal");
        let csv = write_csv(&store, 0, Path::new("/tmp/out.csv"), &cancel).unwrap_err();
        assert_eq!(csv.code, "internal");
    }
}

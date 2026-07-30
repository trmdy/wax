use serde::{Deserialize, Serialize};

pub const DUMP_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CellType {
    N,
    S,
    B,
    E,
    D,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum CellValue {
    Number(f64),
    Text(String),
    Bool(bool),
}

/// Maximum number of entries an export `overrides` array may carry
/// (v0.2 export-with-overrides contract). Breaching it is `bad_request`
/// naming the cap.
pub const EXPORT_OVERRIDES_CAP: usize = 100_000;

/// One edited cell layered over the read model at export time
/// (`docs/v0.2-overrides-contract.md`). Indices are zero-based absolute;
/// `v: None` clears the cell. Strings are always stored as text (a leading
/// `=` is never reinterpreted as a formula) and numbers are never coerced
/// to dates — the retained format code carries date semantics.
#[derive(Clone, Debug, PartialEq)]
pub struct CellOverride {
    pub sheet: u32,
    pub r: u32,
    pub c: u32,
    pub v: Option<CellValue>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Cell {
    pub r: u32,
    pub c: u32,
    pub t: CellType,
    pub v: Option<CellValue>,
    pub d: Option<String>,
    pub f: Option<String>,
    pub fmt: Option<String>,
    /// Index into [`Document::styles`]. Additive in schema 1: absent (`None`)
    /// serializes to nothing, so pre-W4 dumps stay byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s: Option<u32>,
}

/// Explicit width for one zero-based column, in Excel character units
/// (the `width` attribute of a `<col>` element). Only columns with an
/// explicit width appear; everything else uses the consumer's default.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct ColInfo {
    pub c: u32,
    pub width: f64,
}

/// Basic cell styling for export-a-copy fidelity. Deliberately minimal:
/// anything richer (borders, alignment, gradients, themes) is out of the
/// v1 model and must be reported as dropped by the writer, never silently.
/// Colors are `#RRGGBB`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellStyle {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strike: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Sheet {
    pub name: String,
    pub index: u32,
    pub rows: u32,
    pub cols: u32,
    pub truncated: bool,
    pub merges: Vec<String>,
    pub cells: Vec<Cell>,
    /// Explicit column widths. Additive in schema 1: empty serializes to
    /// nothing, so pre-W4 dumps stay byte-identical.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "colInfos")]
    pub col_infos: Vec<ColInfo>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DumpError {
    pub code: String,
    pub msg: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub schema: u32,
    pub tool: String,
    pub tool_version: String,
    pub file: String,
    pub sha256: String,
    pub ok: bool,
    pub error: Option<DumpError>,
    pub wall_ms: u64,
    pub peak_rss_bytes: Option<u64>,
    pub truncated: bool,
    pub sheets: Vec<Sheet>,
    pub warnings: Vec<String>,
    /// Workbook-wide style table referenced by [`Cell::s`]. Additive in
    /// schema 1: empty serializes to nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub styles: Vec<CellStyle>,
}

impl Document {
    pub fn success(
        tool_version: impl Into<String>,
        file: impl Into<String>,
        sheets: Vec<Sheet>,
        warnings: Vec<String>,
    ) -> Self {
        let truncated = sheets.iter().any(|sheet| sheet.truncated);
        Self {
            schema: DUMP_SCHEMA_VERSION,
            tool: "wax".to_owned(),
            tool_version: tool_version.into(),
            file: file.into(),
            sha256: String::new(),
            ok: true,
            error: None,
            wall_ms: 0,
            peak_rss_bytes: None,
            truncated,
            sheets,
            warnings,
            styles: Vec::new(),
        }
    }

    pub fn failure(
        tool_version: impl Into<String>,
        file: impl Into<String>,
        error: DumpError,
        warnings: Vec<String>,
    ) -> Self {
        Self {
            schema: DUMP_SCHEMA_VERSION,
            tool: "wax".to_owned(),
            tool_version: tool_version.into(),
            file: file.into(),
            sha256: String::new(),
            ok: false,
            error: Some(error),
            wall_ms: 0,
            peak_rss_bytes: None,
            truncated: false,
            sheets: Vec::new(),
            warnings,
            styles: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> Document {
        Document {
            schema: DUMP_SCHEMA_VERSION,
            tool: "wax".to_owned(),
            tool_version: "0.1.0".to_owned(),
            file: "corpus/files/tiny.xlsx".to_owned(),
            sha256: "abc123".to_owned(),
            ok: true,
            error: None,
            wall_ms: 12,
            peak_rss_bytes: None,
            truncated: false,
            sheets: vec![Sheet {
                name: "Costs".to_owned(),
                index: 0,
                rows: 1,
                cols: 1,
                truncated: false,
                merges: Vec::new(),
                cells: vec![Cell {
                    r: 0,
                    c: 0,
                    t: CellType::N,
                    v: Some(CellValue::Number(12.5)),
                    d: None,
                    f: None,
                    fmt: None,
                    s: None,
                }],
                col_infos: Vec::new(),
            }],
            warnings: Vec::new(),
            styles: Vec::new(),
        }
    }

    #[test]
    fn serialization_has_exact_contract_fields_and_nulls() {
        let json = serde_json::to_string(&document()).expect("document should serialize");
        assert_eq!(
            json,
            concat!(
                r#"{"schema":1,"tool":"wax","toolVersion":"0.1.0","#,
                r#""file":"corpus/files/tiny.xlsx","sha256":"abc123","#,
                r#""ok":true,"error":null,"wallMs":12,"peakRssBytes":null,"#,
                r#""truncated":false,"sheets":[{"name":"Costs","index":0,"#,
                r#""rows":1,"cols":1,"truncated":false,"merges":[],"cells":[{"#,
                r#""r":0,"c":0,"t":"n","v":12.5,"d":null,"f":null,"fmt":null}]}],"#,
                r#""warnings":[]}"#
            )
        );
    }

    #[test]
    fn style_fields_are_invisible_when_absent_and_round_trip_when_present() {
        // Absent W4 fields must keep pre-W4 dumps byte-identical; the exact
        // serialization is asserted by the test above. Present fields must
        // serialize additively and round-trip.
        let mut expected = document();
        expected.styles = vec![CellStyle {
            bold: true,
            font_size: Some(11.0),
            fill_color: Some("#FFCC00".to_owned()),
            ..CellStyle::default()
        }];
        expected.sheets[0].cells[0].s = Some(0);
        expected.sheets[0].col_infos = vec![ColInfo { c: 2, width: 17.25 }];

        let json = serde_json::to_string(&expected).expect("document should serialize");
        assert!(json.contains(r#""s":0"#));
        assert!(json.contains(r#""colInfos":[{"c":2,"width":17.25}]"#));
        assert!(
            json.contains(r##""styles":[{"bold":true,"fontSize":11.0,"fillColor":"#FFCC00"}]"##)
        );
        assert!(!json.contains("italic"));

        let actual: Document = serde_json::from_str(&json).expect("document should deserialize");
        assert_eq!(actual, expected);
    }

    #[test]
    fn document_round_trips_without_losing_null_fields() {
        let expected = document();
        let json = serde_json::to_string(&expected).expect("document should serialize");
        let actual: Document = serde_json::from_str(&json).expect("document should deserialize");
        assert_eq!(actual, expected);
    }

    #[test]
    fn failure_document_has_contract_defaults() {
        let document = Document::failure(
            "0.1.0",
            "bad.xls",
            DumpError {
                code: "unsupported".to_owned(),
                msg: "unsupported file type".to_owned(),
            },
            vec!["warning".to_owned()],
        );

        assert_eq!(document.schema, 1);
        assert!(!document.ok);
        assert!(document.sheets.is_empty());
        assert_eq!(
            serde_json::to_value(document).expect("failure should serialize")["error"]["code"],
            "unsupported"
        );
    }
}

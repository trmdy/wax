use serde::{Deserialize, Serialize};

pub const DUMP_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Cell {
    pub r: u32,
    pub c: u32,
    pub t: CellType,
    pub v: Option<CellValue>,
    pub d: Option<String>,
    pub f: Option<String>,
    pub fmt: Option<String>,
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
                }],
            }],
            warnings: Vec::new(),
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

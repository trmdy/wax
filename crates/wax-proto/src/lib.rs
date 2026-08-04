use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use wax_core::{
    CellOverride, CellType, CellValue, ColInfo, ColSizeOverride, RowInfo, RowSizeOverride,
    SizeOverrides, EXPORT_OVERRIDES_CAP,
};

pub const PROTO_VERSION: u32 = 0;

/// Capability string advertising the v0.2 export-with-overrides operation.
pub const CAP_EXPORT_OVERRIDES: &str = "exportOverrides";

/// Capability string advertising the v0.3 sheet size info fields
/// (`colInfos`/`rowInfos`/`defaultRowHeight`/`defaultColWidth` on every
/// `open`/`meta` sheet entry; contract pinned at apiary wave/sheet-p2.5).
pub const CAP_SHEET_SIZE_INFOS: &str = "sheetSizeInfos";

/// Capability string advertising the v0.3 export `sizeOverrides` field
/// (column-width/row-height edits applied at export time). Gated
/// separately from `sheetSizeInfos` per the pinned contract.
pub const CAP_EXPORT_SIZE_OVERRIDES: &str = "exportSizeOverrides";

/// Capability string advertising formula evaluation, evaluated wire cells,
/// and the `recalc` operation (v0.4 contract).
pub const CAP_FORMULA_EVAL: &str = "formulaEval";

/// Capability string advertising flat, always-present frozen row/column
/// metadata on every `open`/`meta` sheet entry (v0.4 contract).
pub const CAP_SHEET_VIEW: &str = "sheetView";

/// Capability string advertising optional authored-formula sources (`f`) on
/// recalc/export override entries (v0.5 contract).
pub const CAP_AUTHORED_FORMULAS: &str = "authoredFormulas";

/// Every capability this server advertises on `version` and `open`
/// responses. Additive — absence of `caps` means no capabilities; the
/// `--version` line never carries capabilities (release-workflow contract).
pub fn server_caps() -> Vec<String> {
    vec![
        CAP_EXPORT_OVERRIDES.to_owned(),
        CAP_SHEET_SIZE_INFOS.to_owned(),
        CAP_EXPORT_SIZE_OVERRIDES.to_owned(),
        CAP_FORMULA_EVAL.to_owned(),
        CAP_SHEET_VIEW.to_owned(),
        CAP_AUTHORED_FORMULAS.to_owned(),
    ]
}
pub const SERVE_DEFAULT_MAX_CELLS: u64 = 5_000_000;
pub const SERVE_DEFAULT_MAX_BYTES: u64 = 100 * 1024 * 1024;
pub const SERVE_DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const WINDOW_CELL_CAP: u64 = 262_144;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unsupported,
    BadZip,
    TooLarge,
    Timeout,
    Internal,
    Bomb,
    BadRequest,
    BadHandle,
    Cancelled,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::BadZip => "bad_zip",
            Self::TooLarge => "too_large",
            Self::Timeout => "timeout",
            Self::Internal => "internal",
            Self::Bomb => "bomb",
            Self::BadRequest => "bad_request",
            Self::BadHandle => "bad_handle",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "unsupported" => Some(Self::Unsupported),
            "bad_zip" => Some(Self::BadZip),
            "too_large" => Some(Self::TooLarge),
            "timeout" => Some(Self::Timeout),
            "internal" => Some(Self::Internal),
            "bomb" => Some(Self::Bomb),
            "bad_request" => Some(Self::BadRequest),
            "bad_handle" => Some(Self::BadHandle),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Version {
        id: u64,
    },
    Open {
        id: u64,
        path: String,
        max_cells: u64,
        max_bytes: u64,
        timeout_ms: u64,
    },
    Meta {
        id: u64,
        handle: String,
    },
    Window {
        id: u64,
        handle: String,
        sheet: u32,
        r0: u32,
        c0: u32,
        nr: u32,
        nc: u32,
    },
    Export {
        id: u64,
        handle: String,
        format: String,
        out: String,
        sheet: u32,
        overrides: Vec<CellOverride>,
        size_overrides: SizeOverrides,
    },
    Recalc {
        id: u64,
        handle: String,
        overrides: Vec<CellOverride>,
    },
    Close {
        id: u64,
        handle: String,
    },
    Cancel {
        id: u64,
        target: u64,
    },
    Stats {
        id: u64,
    },
}

impl Request {
    pub const fn id(&self) -> u64 {
        match self {
            Self::Version { id }
            | Self::Open { id, .. }
            | Self::Meta { id, .. }
            | Self::Window { id, .. }
            | Self::Export { id, .. }
            | Self::Recalc { id, .. }
            | Self::Close { id, .. }
            | Self::Cancel { id, .. }
            | Self::Stats { id } => *id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestError {
    pub id: Option<u64>,
    pub msg: String,
}

impl RequestError {
    fn new(id: Option<u64>, msg: impl Into<String>) -> Self {
        Self {
            id,
            msg: msg.into(),
        }
    }
}

pub fn parse_request(line: &str) -> Result<Request, RequestError> {
    let value: Value = serde_json::from_str(line)
        .map_err(|error| RequestError::new(None, format!("invalid JSON: {error}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| RequestError::new(None, "request must be a JSON object"))?;
    let id = object.get("id").and_then(Value::as_u64);
    let id = id.ok_or_else(|| RequestError::new(None, "request id must be a u64"))?;
    let op = required_string(object, id, "op")?;

    match op {
        "version" => Ok(Request::Version { id }),
        "open" => Ok(Request::Open {
            id,
            path: required_string(object, id, "path")?.to_owned(),
            max_cells: optional_u64(object, id, "maxCells", SERVE_DEFAULT_MAX_CELLS)?,
            max_bytes: optional_u64(object, id, "maxBytes", SERVE_DEFAULT_MAX_BYTES)?,
            timeout_ms: optional_u64(object, id, "timeoutMs", SERVE_DEFAULT_TIMEOUT_MS)?,
        }),
        "meta" => Ok(Request::Meta {
            id,
            handle: required_string(object, id, "handle")?.to_owned(),
        }),
        "window" => {
            let nr = required_u32(object, id, "nr")?;
            let nc = required_u32(object, id, "nc")?;
            if nr == 0 || nc == 0 {
                return Err(RequestError::new(
                    Some(id),
                    "window nr and nc must both be at least 1",
                ));
            }
            if u64::from(nr) * u64::from(nc) > WINDOW_CELL_CAP {
                return Err(RequestError::new(
                    Some(id),
                    format!("window exceeds the {WINDOW_CELL_CAP}-cell cap"),
                ));
            }
            Ok(Request::Window {
                id,
                handle: required_string(object, id, "handle")?.to_owned(),
                sheet: required_u32(object, id, "sheet")?,
                r0: required_u32(object, id, "r0")?,
                c0: required_u32(object, id, "c0")?,
                nr,
                nc,
            })
        }
        "export" => Ok(Request::Export {
            id,
            handle: required_string(object, id, "handle")?.to_owned(),
            format: required_string(object, id, "format")?.to_owned(),
            out: required_string(object, id, "out")?.to_owned(),
            sheet: optional_u32(object, id, "sheet", 0)?,
            overrides: match object.get("overrides") {
                None => Vec::new(),
                Some(value) => parse_overrides(value)
                    .map_err(|message| RequestError::new(Some(id), message))?,
            },
            size_overrides: match object.get("sizeOverrides") {
                None => SizeOverrides::default(),
                Some(value) => parse_size_overrides(value)
                    .map_err(|message| RequestError::new(Some(id), message))?,
            },
        }),
        "recalc" => Ok(Request::Recalc {
            id,
            handle: required_string(object, id, "handle")?.to_owned(),
            overrides: match object.get("overrides") {
                None => Vec::new(),
                Some(value) => parse_overrides(value)
                    .map_err(|message| RequestError::new(Some(id), message))?,
            },
        }),
        "close" => Ok(Request::Close {
            id,
            handle: required_string(object, id, "handle")?.to_owned(),
        }),
        "cancel" => Ok(Request::Cancel {
            id,
            target: required_u64(object, id, "target")?,
        }),
        "stats" => Ok(Request::Stats { id }),
        unknown => Err(RequestError::new(
            Some(id),
            format!("unknown op {unknown:?}"),
        )),
    }
}

/// Parse a recalc/export `overrides` value. Every entry has zero-based u32
/// coordinates and either a literal `v` (the frozen v0.2 shape) or an
/// authored formula string `f` with an optional advisory `v` cache. Errors
/// are `bad_request`-grade messages naming the cap or offending entry/field
/// (contract amendment A5). Also used by the CLI `export --overrides` path,
/// so the message carries no request id.
pub fn parse_overrides(value: &Value) -> Result<Vec<CellOverride>, String> {
    let entries = value
        .as_array()
        .ok_or_else(|| "overrides must be an array".to_owned())?;
    if entries.len() > EXPORT_OVERRIDES_CAP {
        return Err(format!(
            "overrides length {} exceeds the {EXPORT_OVERRIDES_CAP}-entry cap",
            entries.len()
        ));
    }
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let object = entry
                .as_object()
                .ok_or_else(|| format!("overrides[{index}] must be an object"))?;
            let coordinate = |field: &str| -> Result<u32, String> {
                object
                    .get(field)
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| format!("overrides[{index}].{field} must be a u32"))
            };
            let f = match object.get("f") {
                Some(Value::String(formula)) => Some(formula.clone()),
                Some(_) => {
                    return Err(format!("overrides[{index}].f must be a string"));
                }
                None => None,
            };
            let v = match object.get("v") {
                Some(Value::Null) => None,
                Some(Value::Number(number)) => {
                    let number = number
                        .as_f64()
                        .filter(|number| number.is_finite())
                        .ok_or_else(|| format!("overrides[{index}].v must be a finite number"))?;
                    Some(CellValue::Number(number))
                }
                Some(Value::String(text)) => Some(CellValue::Text(text.clone())),
                Some(Value::Bool(boolean)) => Some(CellValue::Bool(*boolean)),
                None if f.is_some() => None,
                None | Some(Value::Array(_)) | Some(Value::Object(_)) => {
                    return Err(format!(
                        "overrides[{index}].v must be a number, string, boolean, or null"
                    ))
                }
            };
            Ok(CellOverride {
                sheet: coordinate("sheet")?,
                r: coordinate("r")?,
                c: coordinate("c")?,
                v,
                f,
            })
        })
        .collect()
}

/// Parse an export `sizeOverrides` value: an object with optional `cols`
/// (`[{sheet, c, width}]`) and `rows` (`[{sheet, r, height}]`) arrays.
/// Indices are zero-based u32; widths are character units and heights are
/// points, both finite numbers (the writer clamps out-of-range values
/// loudly). The combined entry count shares the cell-override cap. Errors
/// are `bad_request`-grade messages naming the cap or the offending field.
pub fn parse_size_overrides(value: &Value) -> Result<SizeOverrides, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "sizeOverrides must be an object".to_owned())?;
    for key in object.keys() {
        if key != "cols" && key != "rows" {
            return Err(format!("sizeOverrides has unknown field {key:?}"));
        }
    }
    let entries = |key: &str| -> Result<&[Value], String> {
        match object.get(key) {
            None => Ok(&[]),
            Some(value) => value
                .as_array()
                .map(Vec::as_slice)
                .ok_or_else(|| format!("sizeOverrides.{key} must be an array")),
        }
    };
    let cols = entries("cols")?;
    let rows = entries("rows")?;
    if cols.len().saturating_add(rows.len()) > EXPORT_OVERRIDES_CAP {
        return Err(format!(
            "sizeOverrides length {} exceeds the {EXPORT_OVERRIDES_CAP}-entry cap",
            cols.len().saturating_add(rows.len())
        ));
    }

    let field_u32 = |entry: &Value, key: &str, field: &str, index: usize| -> Result<u32, String> {
        entry
            .as_object()
            .and_then(|object| object.get(field))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| format!("sizeOverrides.{key}[{index}].{field} must be a u32"))
    };
    let field_f64 = |entry: &Value, key: &str, field: &str, index: usize| -> Result<f64, String> {
        entry
            .as_object()
            .ok_or_else(|| format!("sizeOverrides.{key}[{index}] must be an object"))?
            .get(field)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("sizeOverrides.{key}[{index}].{field} must be a finite number"))
    };

    Ok(SizeOverrides {
        cols: cols
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                Ok(ColSizeOverride {
                    width: field_f64(entry, "cols", "width", index)?,
                    sheet: field_u32(entry, "cols", "sheet", index)?,
                    c: field_u32(entry, "cols", "c", index)?,
                })
            })
            .collect::<Result<_, String>>()?,
        rows: rows
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                Ok(RowSizeOverride {
                    height: field_f64(entry, "rows", "height", index)?,
                    sheet: field_u32(entry, "rows", "sheet", index)?,
                    r: field_u32(entry, "rows", "r", index)?,
                })
            })
            .collect::<Result<_, String>>()?,
    })
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    id: u64,
    field: &str,
) -> Result<&'a str, RequestError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| RequestError::new(Some(id), format!("{field} must be a string")))
}

fn required_u64(object: &Map<String, Value>, id: u64, field: &str) -> Result<u64, RequestError> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| RequestError::new(Some(id), format!("{field} must be a u64")))
}

fn optional_u64(
    object: &Map<String, Value>,
    id: u64,
    field: &str,
    default: u64,
) -> Result<u64, RequestError> {
    match object.get(field) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .ok_or_else(|| RequestError::new(Some(id), format!("{field} must be a u64"))),
    }
}

fn required_u32(object: &Map<String, Value>, id: u64, field: &str) -> Result<u32, RequestError> {
    let value = required_u64(object, id, field)?;
    u32::try_from(value)
        .map_err(|_| RequestError::new(Some(id), format!("{field} exceeds u32 range")))
}

fn optional_u32(
    object: &Map<String, Value>,
    id: u64,
    field: &str,
    default: u32,
) -> Result<u32, RequestError> {
    match object.get(field) {
        None => Ok(default),
        Some(value) => {
            let value = value
                .as_u64()
                .ok_or_else(|| RequestError::new(Some(id), format!("{field} must be a u32")))?;
            u32::try_from(value)
                .map_err(|_| RequestError::new(Some(id), format!("{field} exceeds u32 range")))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Response {
    Error(ErrorResponse),
    Version(VersionResponse),
    Open(OpenResponse),
    Meta(MetaResponse),
    Window(WindowResponse),
    Export(ExportResponse),
    Recalc(RecalcResponse),
    Close(CloseResponse),
    Cancel(CancelResponse),
    Stats(StatsResponse),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ErrorResponse {
    pub id: Option<u64>,
    pub ok: bool,
    pub code: ErrorCode,
    pub msg: String,
}

impl ErrorResponse {
    pub fn new(id: Option<u64>, code: ErrorCode, msg: impl Into<String>) -> Self {
        Self {
            id,
            ok: false,
            code,
            msg: msg.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VersionResponse {
    pub id: u64,
    pub ok: bool,
    pub proto: u32,
    pub version: String,
    pub caps: Vec<String>,
}

/// One `open`/`meta` sheet entry. Under the `sheetSizeInfos` capability all
/// four size fields are always present: the arrays may be empty (no
/// non-default entries) and the defaults are always concrete — container
/// declarations when present, otherwise the Excel fallbacks (15.0 points /
/// 8.43 character units), so consumers never need their own fallback.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SheetSummary {
    pub name: String,
    pub rows: u32,
    pub cols: u32,
    pub truncated: bool,
    #[serde(rename = "colInfos")]
    pub col_infos: Vec<ColInfo>,
    #[serde(rename = "rowInfos")]
    pub row_infos: Vec<RowInfo>,
    #[serde(rename = "defaultRowHeight")]
    pub default_row_height: f64,
    #[serde(rename = "defaultColWidth")]
    pub default_col_width: f64,
    #[serde(rename = "frozenRows")]
    pub frozen_rows: u32,
    #[serde(rename = "frozenCols")]
    pub frozen_cols: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenResponse {
    pub id: u64,
    pub ok: bool,
    pub proto: u32,
    pub caps: Vec<String>,
    pub handle: String,
    pub truncated: bool,
    pub sheets: Vec<SheetSummary>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MetaResponse {
    pub id: u64,
    pub ok: bool,
    pub truncated: bool,
    pub sheets: Vec<SheetSummary>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WireCell {
    pub t: CellType,
    pub v: Option<CellValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub f: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fmt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WindowResponse {
    pub id: u64,
    pub ok: bool,
    pub sheet: u32,
    pub r0: u32,
    pub c0: u32,
    pub nr: u32,
    pub nc: u32,
    pub rows: Vec<Vec<Option<WireCell>>>,
    pub merges: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExportResponse {
    pub id: u64,
    pub ok: bool,
    pub bytes: u64,
    pub applied: u64,
    pub dropped: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RecalcCell {
    pub sheet: u32,
    pub r: u32,
    pub c: u32,
    pub v: Option<CellValue>,
    pub d: Option<String>,
    pub e: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RecalcResponse {
    pub id: u64,
    pub ok: bool,
    pub changed: Vec<RecalcCell>,
    pub evaluated: u64,
    pub skipped: u64,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CloseResponse {
    pub id: u64,
    pub ok: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CancelResponse {
    pub id: u64,
    pub ok: bool,
    pub found: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsResponse {
    pub id: u64,
    pub ok: bool,
    pub peak_rss_bytes: u64,
    pub handles: usize,
    pub store_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_match_protocol_strings() {
        let codes = [
            (ErrorCode::Unsupported, "unsupported"),
            (ErrorCode::BadZip, "bad_zip"),
            (ErrorCode::TooLarge, "too_large"),
            (ErrorCode::Timeout, "timeout"),
            (ErrorCode::Internal, "internal"),
            (ErrorCode::Bomb, "bomb"),
            (ErrorCode::BadRequest, "bad_request"),
            (ErrorCode::BadHandle, "bad_handle"),
            (ErrorCode::Cancelled, "cancelled"),
        ];

        for (code, expected) in codes {
            assert_eq!(code.as_str(), expected);
            assert_eq!(ErrorCode::from_code(expected), Some(code));
            assert_eq!(
                serde_json::to_string(&code).expect("code should serialize"),
                format!("\"{expected}\"")
            );
        }
        assert_eq!(ErrorCode::from_code("not_a_code"), None);
    }

    #[test]
    fn open_defaults_are_serve_specific() {
        let request = parse_request(r#"{"id":2,"op":"open","path":"book.xlsx"}"#)
            .expect("request should parse");
        assert_eq!(
            request,
            Request::Open {
                id: 2,
                path: "book.xlsx".to_owned(),
                max_cells: 5_000_000,
                max_bytes: 104_857_600,
                timeout_ms: 30_000,
            }
        );
    }

    #[test]
    fn window_validates_dimensions_and_cap() {
        for line in [
            r#"{"id":1,"op":"window","handle":"h1","sheet":0,"r0":0,"c0":0,"nr":0,"nc":1}"#,
            r#"{"id":2,"op":"window","handle":"h1","sheet":0,"r0":0,"c0":0,"nr":513,"nc":512}"#,
            r#"{"id":3,"op":"window","handle":"h1","sheet":0,"r0":4294967296,"c0":0,"nr":1,"nc":1}"#,
        ] {
            let error = parse_request(line).expect_err("request should fail");
            assert!(error.id.is_some());
        }
        assert!(parse_request(
            r#"{"id":4,"op":"window","handle":"h1","sheet":0,"r0":0,"c0":0,"nr":512,"nc":512}"#
        )
        .is_ok());
    }

    #[test]
    fn malformed_or_bad_ids_have_null_response_id() {
        for line in [
            "{",
            "[]",
            r#"{"op":"version"}"#,
            r#"{"id":-1,"op":"version"}"#,
            r#"{"id":1.5,"op":"version"}"#,
            r#"{"id":"1","op":"version"}"#,
        ] {
            assert_eq!(
                parse_request(line).expect_err("request should fail").id,
                None
            );
        }
    }

    #[test]
    fn unknown_op_preserves_usable_id() {
        let error = parse_request(r#"{"id":42,"op":"wat"}"#).expect_err("request should fail");
        assert_eq!(error.id, Some(42));
        assert!(error.msg.contains("unknown op"));
    }

    #[test]
    fn optional_numbers_reject_null_and_wrong_types() {
        for line in [
            r#"{"id":1,"op":"open","path":"x.xlsx","maxCells":null}"#,
            r#"{"id":2,"op":"open","path":"x.xlsx","maxBytes":"10"}"#,
            r#"{"id":3,"op":"open","path":"x.xlsx","timeoutMs":-1}"#,
        ] {
            assert!(parse_request(line).is_err());
        }
    }

    #[test]
    fn export_overrides_parse_each_value_kind_and_default_empty() {
        let request = parse_request(
            r#"{"id":9,"op":"export","handle":"h1","format":"csv","out":"/tmp/x.csv",
                "overrides":[
                    {"sheet":0,"r":1,"c":2,"v":42.5},
                    {"sheet":1,"r":0,"c":0,"v":"=SUM(A1:A2)"},
                    {"sheet":0,"r":3,"c":4,"v":true},
                    {"sheet":0,"r":5,"c":6,"v":null}
                ]}"#,
        )
        .expect("request should parse");
        let Request::Export { overrides, .. } = request else {
            panic!("expected export request");
        };
        assert_eq!(
            overrides,
            vec![
                CellOverride {
                    sheet: 0,
                    r: 1,
                    c: 2,
                    v: Some(CellValue::Number(42.5)),
                    f: None,
                },
                CellOverride {
                    sheet: 1,
                    r: 0,
                    c: 0,
                    v: Some(CellValue::Text("=SUM(A1:A2)".to_owned())),
                    f: None,
                },
                CellOverride {
                    sheet: 0,
                    r: 3,
                    c: 4,
                    v: Some(CellValue::Bool(true)),
                    f: None,
                },
                CellOverride {
                    sheet: 0,
                    r: 5,
                    c: 6,
                    v: None,
                    f: None,
                },
            ]
        );

        let request = parse_request(
            r#"{"id":10,"op":"export","handle":"h1","format":"csv","out":"/tmp/x.csv"}"#,
        )
        .expect("request should parse");
        let Request::Export { overrides, .. } = request else {
            panic!("expected export request");
        };
        assert!(overrides.is_empty());
    }

    #[test]
    fn authored_formula_overrides_accept_optional_advisory_values() {
        let overrides = parse_overrides(&serde_json::json!([
            {"sheet":0,"r":1,"c":2,"f":"=SUM(A1:A2)"},
            {"sheet":0,"r":2,"c":2,"f":"=A1*2","v":999}
        ]))
        .expect("authored formulas should parse");
        assert_eq!(overrides[0].f.as_deref(), Some("=SUM(A1:A2)"));
        assert_eq!(overrides[0].v, None);
        assert_eq!(overrides[1].f.as_deref(), Some("=A1*2"));
        assert_eq!(overrides[1].v, Some(CellValue::Number(999.0)));

        // The discriminator is `f`, never a leading equals in literal `v`.
        let literal = parse_overrides(&serde_json::json!([
            {"sheet":0,"r":0,"c":0,"v":"=A1*2"}
        ]))
        .expect("equals-prefixed strings remain literals");
        assert_eq!(literal[0].f, None);
        assert_eq!(literal[0].v, Some(CellValue::Text("=A1*2".to_owned())));
    }

    #[test]
    fn export_overrides_reject_malformed_entries_naming_the_field() {
        for (payload, expected) in [
            (r#""overrides":{}"#, "overrides must be an array"),
            (r#""overrides":[7]"#, "overrides[0] must be an object"),
            (
                r#""overrides":[{"r":0,"c":0,"v":1}]"#,
                "overrides[0].sheet must be a u32",
            ),
            (
                r#""overrides":[{"sheet":0,"r":-1,"c":0,"v":1}]"#,
                "overrides[0].r must be a u32",
            ),
            (
                r#""overrides":[{"sheet":0,"r":0,"c":4294967296,"v":1}]"#,
                "overrides[0].c must be a u32",
            ),
            (
                r#""overrides":[{"sheet":0,"r":0,"c":0}]"#,
                "overrides[0].v must be a number, string, boolean, or null",
            ),
            (
                r#""overrides":[{"sheet":0,"r":0,"c":0,"v":[1]}]"#,
                "overrides[0].v must be a number, string, boolean, or null",
            ),
            (
                r#""overrides":[{"sheet":0,"r":0,"c":0,"v":{}}]"#,
                "overrides[0].v must be a number, string, boolean, or null",
            ),
            (
                r#""overrides":[{"sheet":0,"r":0,"c":0,"f":7}]"#,
                "overrides[0].f must be a string",
            ),
            (
                r#""overrides":[{"sheet":0,"r":0,"c":0,"f":"=1","v":[]}]"#,
                "overrides[0].v must be a number, string, boolean, or null",
            ),
        ] {
            let line = format!(
                r#"{{"id":3,"op":"export","handle":"h1","format":"csv","out":"/tmp/x.csv",{payload}}}"#
            );
            let error = parse_request(&line).expect_err("request should fail");
            assert_eq!(error.id, Some(3), "{payload}");
            assert_eq!(error.msg, expected, "{payload}");
        }

        // Non-finite numbers cannot arrive over the wire at all: serde_json
        // rejects out-of-range literals at the syntax level.
        let error = parse_request(
            r#"{"id":3,"op":"export","handle":"h1","format":"csv","out":"/tmp/x.csv",
                "overrides":[{"sheet":0,"r":0,"c":0,"v":1e999}]}"#,
        )
        .expect_err("out-of-range number should fail");
        assert_eq!(error.id, None);
    }

    #[test]
    fn export_overrides_cap_is_named_in_the_error() {
        let entries = (0..EXPORT_OVERRIDES_CAP + 1)
            .map(|_| serde_json::json!({"sheet":0,"r":0,"c":0,"v":1}))
            .collect::<Vec<_>>();
        let error =
            parse_overrides(&Value::Array(entries)).expect_err("over-cap overrides should fail");
        assert_eq!(
            error,
            format!("overrides length {} exceeds the 100000-entry cap", 100_001)
        );

        let at_cap = (0..EXPORT_OVERRIDES_CAP)
            .map(|_| serde_json::json!({"sheet":0,"r":0,"c":0,"v":1}))
            .collect::<Vec<_>>();
        assert!(parse_overrides(&Value::Array(at_cap)).is_ok());
    }

    #[test]
    fn recalc_reuses_override_shape_and_serializes_evaluated_changes() {
        let request = parse_request(
            r#"{"id":17,"op":"recalc","handle":"h1","overrides":[
                {"sheet":0,"r":1,"c":2,"v":42.5},
                {"sheet":0,"r":1,"c":2,"v":null}
            ]}"#,
        )
        .expect("recalc request should parse");
        let Request::Recalc {
            id,
            handle,
            overrides,
        } = request
        else {
            panic!("expected recalc request");
        };
        assert_eq!(id, 17);
        assert_eq!(handle, "h1");
        assert_eq!(overrides.len(), 2);
        assert_eq!(overrides[0].v, Some(CellValue::Number(42.5)));
        assert_eq!(overrides[1].v, None);
        assert_eq!(overrides[1].f, None);

        let response = serde_json::to_value(RecalcResponse {
            id: 17,
            ok: true,
            changed: vec![RecalcCell {
                sheet: 0,
                r: 1,
                c: 3,
                v: Some(CellValue::Number(43.5)),
                d: Some("43.50".to_owned()),
                e: true,
            }],
            evaluated: 1,
            skipped: 0,
            truncated: false,
            warnings: Vec::new(),
        })
        .expect("recalc response should serialize");
        assert_eq!(
            response,
            serde_json::json!({
                "id":17,"ok":true,
                "changed":[{"sheet":0,"r":1,"c":3,"v":43.5,"d":"43.50","e":true}],
                "evaluated":1,"skipped":0,"truncated":false,"warnings":[]
            })
        );
    }

    #[test]
    fn export_size_overrides_parse_and_reject_malformed_fields() {
        let request = parse_request(
            r#"{"id":7,"op":"export","handle":"h1","format":"xlsx","out":"/tmp/x.xlsx",
                "sizeOverrides":{
                    "cols":[{"sheet":0,"c":2,"width":24.5}],
                    "rows":[{"sheet":1,"r":9,"height":30.6}]
                }}"#,
        )
        .expect("request should parse");
        let Request::Export { size_overrides, .. } = request else {
            panic!("expected export request");
        };
        assert_eq!(
            size_overrides,
            SizeOverrides {
                cols: vec![ColSizeOverride {
                    sheet: 0,
                    c: 2,
                    width: 24.5,
                }],
                rows: vec![RowSizeOverride {
                    sheet: 1,
                    r: 9,
                    height: 30.6,
                }],
            }
        );

        let request = parse_request(
            r#"{"id":8,"op":"export","handle":"h1","format":"xlsx","out":"/tmp/x.xlsx"}"#,
        )
        .expect("request should parse");
        let Request::Export { size_overrides, .. } = request else {
            panic!("expected export request");
        };
        assert!(size_overrides.is_empty());

        for (payload, expected) in [
            (r#""sizeOverrides":[]"#, "sizeOverrides must be an object"),
            (
                r#""sizeOverrides":{"widths":[]}"#,
                "sizeOverrides has unknown field \"widths\"",
            ),
            (
                r#""sizeOverrides":{"cols":{}}"#,
                "sizeOverrides.cols must be an array",
            ),
            (
                r#""sizeOverrides":{"cols":[7]}"#,
                "sizeOverrides.cols[0] must be an object",
            ),
            (
                r#""sizeOverrides":{"cols":[{"sheet":0,"c":1}]}"#,
                "sizeOverrides.cols[0].width must be a finite number",
            ),
            (
                r#""sizeOverrides":{"cols":[{"sheet":0,"c":1,"width":"9"}]}"#,
                "sizeOverrides.cols[0].width must be a finite number",
            ),
            (
                r#""sizeOverrides":{"rows":[{"sheet":0,"r":-1,"height":9}]}"#,
                "sizeOverrides.rows[0].r must be a u32",
            ),
            (
                r#""sizeOverrides":{"rows":[{"r":0,"height":9}]}"#,
                "sizeOverrides.rows[0].sheet must be a u32",
            ),
        ] {
            let line = format!(
                r#"{{"id":3,"op":"export","handle":"h1","format":"xlsx","out":"/tmp/x.xlsx",{payload}}}"#
            );
            let error = parse_request(&line).expect_err("request should fail");
            assert_eq!(error.msg, expected, "{payload}");
        }
    }

    #[test]
    fn export_size_overrides_share_the_overrides_cap() {
        let cols = (0..60_000)
            .map(|_| serde_json::json!({"sheet":0,"c":0,"width":9}))
            .collect::<Vec<_>>();
        let rows = (0..40_001)
            .map(|_| serde_json::json!({"sheet":0,"r":0,"height":15}))
            .collect::<Vec<_>>();
        let error = parse_size_overrides(&serde_json::json!({"cols": cols, "rows": rows}))
            .expect_err("over-cap size overrides should fail");
        assert_eq!(
            error,
            "sizeOverrides length 100001 exceeds the 100000-entry cap"
        );
    }

    #[test]
    fn caps_are_advertised_on_version_and_open_responses_only() {
        assert_eq!(
            server_caps(),
            vec![
                "exportOverrides".to_owned(),
                "sheetSizeInfos".to_owned(),
                "exportSizeOverrides".to_owned(),
                "formulaEval".to_owned(),
                "sheetView".to_owned(),
                "authoredFormulas".to_owned()
            ]
        );
        let version = serde_json::to_value(VersionResponse {
            id: 1,
            ok: true,
            proto: PROTO_VERSION,
            version: "0.2.0".to_owned(),
            caps: server_caps(),
        })
        .expect("version response should serialize");
        assert_eq!(
            version["caps"],
            serde_json::json!([
                "exportOverrides",
                "sheetSizeInfos",
                "exportSizeOverrides",
                "formulaEval",
                "sheetView",
                "authoredFormulas"
            ])
        );
        let open = serde_json::to_value(OpenResponse {
            id: 2,
            ok: true,
            proto: PROTO_VERSION,
            caps: server_caps(),
            handle: "h1".to_owned(),
            truncated: false,
            sheets: Vec::new(),
            warnings: Vec::new(),
        })
        .expect("open response should serialize");
        assert_eq!(
            open["caps"],
            serde_json::json!([
                "exportOverrides",
                "sheetSizeInfos",
                "exportSizeOverrides",
                "formulaEval",
                "sheetView",
                "authoredFormulas"
            ])
        );
    }

    #[test]
    fn sheet_summaries_always_carry_all_four_size_fields() {
        let summary = serde_json::to_value(SheetSummary {
            name: "Costs".to_owned(),
            rows: 3,
            cols: 2,
            truncated: false,
            col_infos: vec![ColInfo { c: 1, width: 24.5 }],
            row_infos: vec![RowInfo {
                r: 0,
                height: 27.75,
            }],
            default_row_height: 14.4,
            default_col_width: 8.43,
            frozen_rows: 2,
            frozen_cols: 1,
        })
        .expect("summary should serialize");
        assert_eq!(
            summary,
            serde_json::json!({
                "name": "Costs",
                "rows": 3,
                "cols": 2,
                "truncated": false,
                "colInfos": [{"c": 1, "width": 24.5}],
                "rowInfos": [{"r": 0, "height": 27.75}],
                "defaultRowHeight": 14.4,
                "defaultColWidth": 8.43,
                "frozenRows": 2,
                "frozenCols": 1,
            })
        );

        // Empty arrays stay present — the sheetSizeInfos contract has no
        // absent-field ambiguity.
        let bare = serde_json::to_value(SheetSummary {
            name: "Empty".to_owned(),
            rows: 0,
            cols: 0,
            truncated: false,
            col_infos: Vec::new(),
            row_infos: Vec::new(),
            default_row_height: 15.0,
            default_col_width: 8.43,
            frozen_rows: 0,
            frozen_cols: 0,
        })
        .expect("summary should serialize");
        assert_eq!(bare["colInfos"], serde_json::json!([]));
        assert_eq!(bare["rowInfos"], serde_json::json!([]));
    }

    #[test]
    fn wire_cell_keeps_null_value_but_omits_other_null_fields() {
        let cell = WireCell {
            t: CellType::N,
            v: None,
            d: None,
            f: None,
            fmt: None,
            e: None,
        };
        assert_eq!(
            serde_json::to_value(cell).expect("cell should serialize"),
            serde_json::json!({"t":"n","v":null})
        );

        let evaluated = WireCell {
            t: CellType::N,
            v: Some(CellValue::Number(5.0)),
            d: Some("5".to_owned()),
            f: Some("SUM(A1:B1)".to_owned()),
            fmt: None,
            e: Some(true),
        };
        assert_eq!(
            serde_json::to_value(evaluated).expect("evaluated cell should serialize"),
            serde_json::json!({"t":"n","v":5.0,"d":"5","f":"SUM(A1:B1)","e":true})
        );
    }
}

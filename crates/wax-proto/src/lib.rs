use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use wax_core::{CellType, CellValue};

pub const PROTO_VERSION: u32 = 0;
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

#[derive(Clone, Debug, Eq, PartialEq)]
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SheetSummary {
    pub name: String,
    pub rows: u32,
    pub cols: u32,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenResponse {
    pub id: u64,
    pub ok: bool,
    pub proto: u32,
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
    pub dropped: Vec<String>,
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
    fn wire_cell_keeps_null_value_but_omits_other_null_fields() {
        let cell = WireCell {
            t: CellType::N,
            v: None,
            d: None,
            f: None,
            fmt: None,
        };
        assert_eq!(
            serde_json::to_value(cell).expect("cell should serialize"),
            serde_json::json!({"t":"n","v":null})
        );
    }
}

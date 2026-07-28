use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const DOCUMENT_FIELDS: &[&str] = &[
    "schema",
    "tool",
    "toolVersion",
    "file",
    "sha256",
    "ok",
    "error",
    "wallMs",
    "peakRssBytes",
    "truncated",
    "sheets",
    "warnings",
];
const SHEET_FIELDS: &[&str] = &[
    "name",
    "index",
    "rows",
    "cols",
    "truncated",
    "merges",
    "cells",
];
const CELL_FIELDS: &[&str] = &["r", "c", "t", "v", "d", "f", "fmt"];
const ERROR_FIELDS: &[&str] = &["code", "msg"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Wax,
    Sheetjs,
}

impl fmt::Display for Tool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wax => f.write_str("wax"),
            Self::Sheetjs => f.write_str("sheetjs"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DumpError {
    pub code: String,
    pub msg: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DumpDocument {
    pub schema: u32,
    pub tool: Tool,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sheet {
    pub name: String,
    pub index: usize,
    pub rows: u64,
    pub cols: u64,
    pub truncated: bool,
    pub merges: Vec<String>,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub r: u64,
    pub c: u64,
    pub t: CellType,
    pub v: Value,
    pub d: Option<String>,
    pub f: Option<String>,
    pub fmt: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellType {
    #[serde(rename = "n")]
    Number,
    #[serde(rename = "s")]
    Text,
    #[serde(rename = "b")]
    Bool,
    #[serde(rename = "e")]
    Error,
    #[serde(rename = "d")]
    Date,
}

#[derive(Debug, Clone, Copy)]
pub struct ExpectedDump<'a> {
    pub tool: Tool,
    pub sha256: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    message: String,
}

impl SchemaError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SchemaError {}

impl DumpDocument {
    pub fn parse(input: &[u8], expected: ExpectedDump<'_>) -> Result<Self, SchemaError> {
        let value: Value = serde_json::from_slice(input)
            .map_err(|error| SchemaError::new(format!("invalid JSON: {error}")))?;
        validate_required_fields(&value, "document", DOCUMENT_FIELDS)?;

        let object = value
            .as_object()
            .ok_or_else(|| SchemaError::new("document must be a JSON object"))?;
        if let Some(error) = object.get("error") {
            if !error.is_null() {
                validate_required_fields(error, "document.error", ERROR_FIELDS)?;
            }
        }
        if let Some(sheets) = object.get("sheets").and_then(Value::as_array) {
            for (sheet_index, sheet) in sheets.iter().enumerate() {
                validate_required_fields(
                    sheet,
                    &format!("document.sheets[{sheet_index}]"),
                    SHEET_FIELDS,
                )?;
                if let Some(cells) = sheet.get("cells").and_then(Value::as_array) {
                    for (cell_index, cell) in cells.iter().enumerate() {
                        validate_required_fields(
                            cell,
                            &format!("document.sheets[{sheet_index}].cells[{cell_index}]"),
                            CELL_FIELDS,
                        )?;
                    }
                }
            }
        }

        let document: Self = serde_json::from_value(value)
            .map_err(|error| SchemaError::new(format!("schema mismatch: {error}")))?;
        document.validate(expected)?;
        Ok(document)
    }

    fn validate(&self, expected: ExpectedDump<'_>) -> Result<(), SchemaError> {
        if self.schema != 1 {
            return Err(SchemaError::new(format!(
                "unsupported dump schema {}; expected 1",
                self.schema
            )));
        }
        if self.tool != expected.tool {
            return Err(SchemaError::new(format!(
                "tool is {}; expected {}",
                self.tool, expected.tool
            )));
        }
        if self.tool_version.trim().is_empty() {
            return Err(SchemaError::new("toolVersion must not be empty"));
        }
        if self.file.trim().is_empty() {
            return Err(SchemaError::new("file must not be empty"));
        }
        if self.sha256.trim().is_empty() {
            return Err(SchemaError::new("sha256 must not be empty"));
        }
        if let Some(expected_sha256) = expected.sha256 {
            if self.sha256 != expected_sha256 {
                return Err(SchemaError::new(format!(
                    "sha256 is {}; manifest expected {expected_sha256}",
                    self.sha256
                )));
            }
        }
        match (self.ok, &self.error) {
            (true, Some(_)) => {
                return Err(SchemaError::new("ok:true document must have error:null"));
            }
            (false, None) => {
                return Err(SchemaError::new(
                    "ok:false document must include an error object",
                ));
            }
            _ => {}
        }
        if let Some(error) = &self.error {
            if error.code.trim().is_empty() || error.msg.trim().is_empty() {
                return Err(SchemaError::new("error code and msg must not be empty"));
            }
        }

        for (expected_index, sheet) in self.sheets.iter().enumerate() {
            sheet.validate(expected_index)?;
        }

        let any_sheet_truncated = self.sheets.iter().any(|sheet| sheet.truncated);
        if self.truncated != any_sheet_truncated {
            return Err(SchemaError::new(
                "document truncated flag must equal the union of sheet flags",
            ));
        }
        Ok(())
    }
}

impl Sheet {
    fn validate(&self, expected_index: usize) -> Result<(), SchemaError> {
        let location = format!("sheet[{expected_index}]");
        if self.index != expected_index {
            return Err(SchemaError::new(format!(
                "{location}.index is {}; expected {expected_index}",
                self.index
            )));
        }
        if self.name.is_empty() {
            return Err(SchemaError::new(format!(
                "{location}.name must not be empty"
            )));
        }
        if !strictly_sorted(&self.merges) {
            return Err(SchemaError::new(format!(
                "{location}.merges must be unique and ascending"
            )));
        }

        let mut previous = None;
        for cell in &self.cells {
            if cell.r >= self.rows || cell.c >= self.cols {
                return Err(SchemaError::new(format!(
                    "{location} cell ({}, {}) lies outside extent {}x{}",
                    cell.r, cell.c, self.rows, self.cols
                )));
            }
            let coordinate = (cell.r, cell.c);
            if previous.is_some_and(|value| coordinate <= value) {
                return Err(SchemaError::new(format!(
                    "{location}.cells must be unique and ascending row-major"
                )));
            }
            cell.validate(&location)?;
            previous = Some(coordinate);
        }
        Ok(())
    }
}

impl Cell {
    fn validate(&self, sheet_location: &str) -> Result<(), SchemaError> {
        if let Some(formula) = &self.f {
            if formula.trim().is_empty() {
                return Err(SchemaError::new(format!(
                    "{sheet_location} cell ({}, {}) has an empty formula",
                    self.r, self.c
                )));
            }
            if formula.trim_start().starts_with('=') {
                return Err(SchemaError::new(format!(
                    "{sheet_location} cell ({}, {}) formula must omit the leading =",
                    self.r, self.c
                )));
            }
        }
        if self.v.is_null() {
            if self.f.is_none() {
                return Err(SchemaError::new(format!(
                    "{sheet_location} cell ({}, {}) has null v without a formula",
                    self.r, self.c
                )));
            }
            return Ok(());
        }

        let valid = match self.t {
            CellType::Number => self.v.is_number(),
            CellType::Text | CellType::Error | CellType::Date => self.v.is_string(),
            CellType::Bool => self.v.is_boolean(),
        };
        if !valid {
            return Err(SchemaError::new(format!(
                "{sheet_location} cell ({}, {}) has a value incompatible with its type",
                self.r, self.c
            )));
        }
        Ok(())
    }
}

fn validate_required_fields(
    value: &Value,
    location: &str,
    fields: &[&str],
) -> Result<(), SchemaError> {
    let object = value
        .as_object()
        .ok_or_else(|| SchemaError::new(format!("{location} must be a JSON object")))?;
    for field in fields {
        if !object.contains_key(*field) {
            return Err(SchemaError::new(format!(
                "{location} is missing required field {field}"
            )));
        }
    }
    Ok(())
}

fn strictly_sorted(values: &[String]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::{DumpDocument, ExpectedDump, Tool};

    fn valid_dump() -> Value {
        json!({
            "schema": 1,
            "tool": "wax",
            "toolVersion": "0.1.0",
            "file": "fixture.xlsx",
            "sha256": "abc",
            "ok": true,
            "error": null,
            "wallMs": 4,
            "peakRssBytes": null,
            "truncated": false,
            "sheets": [{
                "name": "Sheet1",
                "index": 0,
                "rows": 1,
                "cols": 1,
                "truncated": false,
                "merges": [],
                "cells": [{"r": 0, "c": 0, "t": "n", "v": 1, "d": null, "f": null, "fmt": null}]
            }],
            "warnings": []
        })
    }

    fn parse(value: &Value) -> Result<DumpDocument, super::SchemaError> {
        DumpDocument::parse(
            serde_json::to_vec(value).unwrap().as_slice(),
            ExpectedDump {
                tool: Tool::Wax,
                sha256: Some("abc"),
            },
        )
    }

    #[test]
    fn parses_contract_document() {
        let document = parse(&valid_dump()).unwrap();
        assert!(document.ok);
        assert_eq!(document.sheets[0].cells.len(), 1);
    }

    #[test]
    fn missing_nullable_field_is_a_schema_error() {
        let mut value = valid_dump();
        value["sheets"][0]["cells"][0]
            .as_object_mut()
            .unwrap()
            .remove("fmt");

        let error = parse(&value).unwrap_err();
        assert!(error.to_string().contains("missing required field fmt"));
    }

    #[test]
    fn unordered_cells_are_a_schema_error() {
        let mut value = valid_dump();
        value["sheets"][0]["rows"] = json!(2);
        value["sheets"][0]["cells"] = json!([
            {"r": 1, "c": 0, "t": "n", "v": 2, "d": null, "f": null, "fmt": null},
            {"r": 0, "c": 0, "t": "n", "v": 1, "d": null, "f": null, "fmt": null}
        ]);

        let error = parse(&value).unwrap_err();
        assert!(error.to_string().contains("ascending row-major"));
    }

    #[test]
    fn type_value_mismatch_is_a_schema_error() {
        let mut value = valid_dump();
        value["sheets"][0]["cells"][0]["t"] = json!("b");

        let error = parse(&value).unwrap_err();
        assert!(error.to_string().contains("incompatible"));
    }
}

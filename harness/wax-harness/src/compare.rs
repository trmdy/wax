use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Cell, CellType, DumpDocument, DumpError, Tool};
use crate::serve::ServeFileMetrics;

#[derive(Debug, Clone)]
pub struct ToolObservation {
    pub tool: Tool,
    pub document: Option<DumpDocument>,
    pub failure: Option<DumpError>,
    pub measured_wall_ms: Option<f64>,
}

impl ToolObservation {
    pub fn document(document: DumpDocument) -> Self {
        Self {
            tool: document.tool,
            document: Some(document),
            failure: None,
            measured_wall_ms: None,
        }
    }

    pub fn failure(
        tool: Tool,
        code: impl Into<String>,
        msg: impl Into<String>,
        measured_wall_ms: Option<f64>,
    ) -> Self {
        Self {
            tool,
            document: None,
            failure: Some(DumpError {
                code: code.into(),
                msg: msg.into(),
            }),
            measured_wall_ms,
        }
    }

    fn summary(&self) -> ToolSummary {
        match &self.document {
            Some(document) => ToolSummary {
                ok: document.ok,
                error: document.error.clone(),
                wall_ms: Some(document.wall_ms),
                peak_rss_bytes: document.peak_rss_bytes,
                truncated: document.truncated,
            },
            None => ToolSummary {
                ok: false,
                error: self.failure.clone().or_else(|| {
                    Some(DumpError {
                        code: "internal_error".to_owned(),
                        msg: format!("{} produced neither a document nor an error", self.tool),
                    })
                }),
                wall_ms: self.measured_wall_ms,
                peak_rss_bytes: None,
                truncated: false,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSummary {
    pub ok: bool,
    pub error: Option<DumpError>,
    pub wall_ms: Option<f64>,
    pub peak_rss_bytes: Option<u64>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountMetric {
    pub matched: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageMetric {
    pub covered: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatDisplayMetric {
    pub code: String,
    pub oracle_cells: u64,
    pub wax_display_coverage: CoverageMetric,
    pub display_string_match: CountMetric,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MismatchBucket {
    pub category: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMetrics {
    pub id: String,
    pub path: String,
    pub sha256: String,
    #[serde(default)]
    pub ext: String,
    #[serde(default)]
    pub private: bool,
    pub wax: ToolSummary,
    pub sheetjs: ToolSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serve: Option<ServeFileMetrics>,
    pub cell_value_match: CountMetric,
    pub wax_display_coverage: CoverageMetric,
    pub sheetjs_display_coverage: CoverageMetric,
    #[serde(default)]
    pub display_string_match: CountMetric,
    pub formula_fidelity: CountMetric,
    pub cached_result_fidelity: CountMetric,
    #[serde(default)]
    pub format_display: Vec<FormatDisplayMetric>,
    #[serde(default)]
    pub value_mismatches: Vec<MismatchBucket>,
    #[serde(default)]
    pub display_mismatches: Vec<MismatchBucket>,
    pub warnings: Vec<String>,
}

pub fn compare(
    id: impl Into<String>,
    path: impl Into<String>,
    sha256: impl Into<String>,
    wax: &ToolObservation,
    sheetjs: &ToolObservation,
) -> FileMetrics {
    let wax_summary = wax.summary();
    let sheetjs_summary = sheetjs.summary();
    let path = path.into();
    let ext = Path::new(&path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut file = FileMetrics {
        id: id.into(),
        path,
        sha256: sha256.into(),
        ext,
        private: false,
        wax: wax_summary,
        sheetjs: sheetjs_summary,
        serve: None,
        cell_value_match: CountMetric::default(),
        wax_display_coverage: coverage(wax.document.as_ref()),
        sheetjs_display_coverage: coverage(sheetjs.document.as_ref()),
        display_string_match: CountMetric::default(),
        formula_fidelity: CountMetric::default(),
        cached_result_fidelity: CountMetric::default(),
        format_display: format_display_metrics(wax.document.as_ref(), sheetjs.document.as_ref()),
        value_mismatches: Vec::new(),
        display_mismatches: Vec::new(),
        warnings: Vec::new(),
    };

    let (Some(wax_dump), Some(sheetjs_dump)) = (&wax.document, &sheetjs.document) else {
        return file;
    };
    if !wax_dump.ok || !sheetjs_dump.ok {
        return file;
    }
    if wax_dump.truncated != sheetjs_dump.truncated {
        file.warnings.push(format!(
            "asymmetric truncation: wax={}, sheetjs={}",
            wax_dump.truncated, sheetjs_dump.truncated
        ));
    }

    let wax_cells = indexed_cells(wax_dump);
    let sheetjs_cells = indexed_cells(sheetjs_dump);
    let mut keys: Vec<_> = wax_cells
        .keys()
        .chain(sheetjs_cells.keys())
        .copied()
        .collect();
    keys.sort_unstable();
    keys.dedup();

    let mut value_mismatches = BTreeMap::new();
    let mut display_mismatches = BTreeMap::new();
    for key in keys {
        let wax_cell = wax_cells.get(&key).copied();
        let sheetjs_cell = sheetjs_cells.get(&key).copied();

        file.cell_value_match.total += 1;
        if cells_have_equal_values(wax_cell, sheetjs_cell) {
            file.cell_value_match.matched += 1;
        } else {
            increment(
                &mut value_mismatches,
                value_mismatch_category(wax_cell, sheetjs_cell),
            );
        }

        if let (Some(wax_display), Some(sheetjs_display)) = (
            wax_cell.and_then(|cell| cell.d.as_deref()),
            sheetjs_cell.and_then(|cell| cell.d.as_deref()),
        ) {
            file.display_string_match.total += 1;
            if wax_display == sheetjs_display {
                file.display_string_match.matched += 1;
            } else {
                increment(
                    &mut display_mismatches,
                    format_category(sheetjs_cell.and_then(|cell| cell.fmt.as_deref())),
                );
            }
        }

        let has_formula = wax_cell.and_then(|cell| cell.f.as_ref()).is_some()
            || sheetjs_cell.and_then(|cell| cell.f.as_ref()).is_some();
        if has_formula {
            file.formula_fidelity.total += 1;
            if formulas_equal(wax_cell, sheetjs_cell) {
                file.formula_fidelity.matched += 1;
            }

            file.cached_result_fidelity.total += 1;
            if cells_have_equal_values(wax_cell, sheetjs_cell) {
                file.cached_result_fidelity.matched += 1;
            }
        }
    }
    file.value_mismatches = buckets(value_mismatches);
    file.display_mismatches = buckets(display_mismatches);

    file
}

fn indexed_cells(document: &DumpDocument) -> BTreeMap<(usize, u64, u64), &Cell> {
    let mut cells = BTreeMap::new();
    for sheet in &document.sheets {
        for cell in &sheet.cells {
            cells.insert((sheet.index, cell.r, cell.c), cell);
        }
    }
    cells
}

fn coverage(document: Option<&DumpDocument>) -> CoverageMetric {
    let Some(document) = document.filter(|document| document.ok) else {
        return CoverageMetric::default();
    };

    document.sheets.iter().flat_map(|sheet| &sheet.cells).fold(
        CoverageMetric::default(),
        |mut metric, cell| {
            metric.total += 1;
            metric.covered += u64::from(cell.d.is_some());
            metric
        },
    )
}

fn format_display_metrics(
    wax: Option<&DumpDocument>,
    sheetjs: Option<&DumpDocument>,
) -> Vec<FormatDisplayMetric> {
    let Some(sheetjs) = sheetjs.filter(|document| document.ok) else {
        return Vec::new();
    };
    let wax_cells = wax
        .filter(|document| document.ok)
        .map(indexed_cells)
        .unwrap_or_default();
    let mut formats = BTreeMap::<String, FormatDisplayMetric>::new();

    for (coordinate, sheetjs_cell) in indexed_cells(sheetjs) {
        let Some(code) = sheetjs_cell.fmt.as_deref() else {
            continue;
        };
        let metric = formats
            .entry(code.to_owned())
            .or_insert_with(|| FormatDisplayMetric {
                code: code.to_owned(),
                ..FormatDisplayMetric::default()
            });
        metric.oracle_cells += 1;
        metric.wax_display_coverage.total += 1;

        let wax_display = wax_cells
            .get(&coordinate)
            .and_then(|cell| cell.d.as_deref());
        if wax_display.is_some() {
            metric.wax_display_coverage.covered += 1;
        }
        if let (Some(wax_display), Some(sheetjs_display)) = (wax_display, sheetjs_cell.d.as_deref())
        {
            metric.display_string_match.total += 1;
            if wax_display == sheetjs_display {
                metric.display_string_match.matched += 1;
            }
        }
    }

    formats.into_values().collect()
}

fn cells_have_equal_values(wax: Option<&Cell>, sheetjs: Option<&Cell>) -> bool {
    match (wax, sheetjs) {
        (Some(wax), Some(sheetjs)) if wax.t == sheetjs.t => values_equal(wax.t, &wax.v, &sheetjs.v),
        _ => false,
    }
}

fn values_equal(cell_type: CellType, left: &Value, right: &Value) -> bool {
    if cell_type != CellType::Number {
        return left == right;
    }
    if left.is_null() || right.is_null() {
        return left.is_null() && right.is_null();
    }

    let (Some(left), Some(right)) = (left.as_f64(), right.as_f64()) else {
        return false;
    };
    if left == right {
        return true;
    }
    let scale = left.abs().max(right.abs());
    scale > 0.0 && (left - right).abs() <= 1e-9 * scale
}

fn formulas_equal(wax: Option<&Cell>, sheetjs: Option<&Cell>) -> bool {
    match (
        wax.and_then(|cell| cell.f.as_deref()),
        sheetjs.and_then(|cell| cell.f.as_deref()),
    ) {
        (Some(wax), Some(sheetjs)) => normalize_formula(wax) == normalize_formula(sheetjs),
        _ => false,
    }
}

fn value_mismatch_category(wax: Option<&Cell>, sheetjs: Option<&Cell>) -> String {
    format!(
        "wax:{} / SheetJS:{}",
        cell_type_label(wax),
        cell_type_label(sheetjs)
    )
}

fn cell_type_label(cell: Option<&Cell>) -> &'static str {
    match cell.map(|cell| cell.t) {
        Some(CellType::Number) => "n",
        Some(CellType::Text) => "s",
        Some(CellType::Bool) => "b",
        Some(CellType::Error) => "e",
        Some(CellType::Date) => "d",
        None => "missing",
    }
}

fn format_category(code: Option<&str>) -> String {
    code.unwrap_or("General").to_owned()
}

fn increment(counts: &mut BTreeMap<String, u64>, category: String) {
    *counts.entry(category).or_default() += 1;
}

fn buckets(counts: BTreeMap<String, u64>) -> Vec<MismatchBucket> {
    counts
        .into_iter()
        .map(|(category, count)| MismatchBucket { category, count })
        .collect()
}

fn normalize_formula(formula: &str) -> String {
    let mut normalized = String::with_capacity(formula.len());
    let mut characters = formula.chars().peekable();
    let mut quote = None;
    let mut bracket_depth = 0_u32;

    while let Some(character) = characters.next() {
        if quote == Some(character) {
            normalized.push(character);
            if characters.peek() == Some(&character) {
                normalized.push(characters.next().expect("peeked character must exist"));
            } else {
                quote = None;
            }
        } else if quote.is_none() && matches!(character, '\'' | '"') {
            quote = Some(character);
            normalized.push(character);
        } else if quote.is_none() && character == '[' {
            bracket_depth = bracket_depth.saturating_add(1);
            normalized.push(character);
        } else if quote.is_none() && character == ']' {
            bracket_depth = bracket_depth.saturating_sub(1);
            normalized.push(character);
        } else if quote.is_some() || bracket_depth > 0 || !character.is_whitespace() {
            normalized.push(character);
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::{compare, ToolObservation};
    use crate::model::{DumpDocument, ExpectedDump, Tool};

    fn dump(tool: Tool, cells: Value, truncated: bool, ok: bool) -> DumpDocument {
        let tool_name = tool.to_string();
        let (error, sheets) = if ok {
            (
                Value::Null,
                json!([{
                    "name": "Sheet1",
                    "index": 0,
                    "rows": 4,
                    "cols": 2,
                    "truncated": truncated,
                    "merges": [],
                    "cells": cells
                }]),
            )
        } else {
            (json!({"code": "unsupported", "msg": "no"}), json!([]))
        };
        let value = json!({
            "schema": 1,
            "tool": tool_name,
            "toolVersion": "0.1.0",
            "file": "fixture.xlsx",
            "sha256": "abc",
            "ok": ok,
            "error": error,
            "wallMs": 10,
            "peakRssBytes": 100,
            "truncated": truncated,
            "sheets": sheets,
            "warnings": []
        });
        DumpDocument::parse(
            serde_json::to_vec(&value).unwrap().as_slice(),
            ExpectedDump {
                tool,
                sha256: Some("abc"),
            },
        )
        .unwrap()
    }

    fn cell(r: u64, t: &str, v: Value, d: Value, f: Value) -> Value {
        json!({"r": r, "c": 0, "t": t, "v": v, "d": d, "f": f, "fmt": null})
    }

    fn formatted_cell(r: u64, v: Value, d: Value, fmt: &str) -> Value {
        json!({"r": r, "c": 0, "t": "n", "v": v, "d": d, "f": null, "fmt": fmt})
    }

    fn compare_cells(wax_cells: Value, sheetjs_cells: Value) -> super::FileMetrics {
        let wax = ToolObservation::document(dump(Tool::Wax, wax_cells, false, true));
        let sheetjs = ToolObservation::document(dump(Tool::Sheetjs, sheetjs_cells, false, true));
        compare("id", "fixture.xlsx", "abc", &wax, &sheetjs)
    }

    #[test]
    fn exact_match_counts_union_and_display_coverage() {
        let metrics = compare_cells(
            json!([cell(0, "n", json!(12.5), Value::Null, Value::Null)]),
            json!([cell(0, "n", json!(12.5), json!("12.5"), Value::Null)]),
        );

        assert_eq!(metrics.cell_value_match.matched, 1);
        assert_eq!(metrics.cell_value_match.total, 1);
        assert_eq!(metrics.wax_display_coverage.covered, 0);
        assert_eq!(metrics.sheetjs_display_coverage.covered, 1);
        assert_eq!(metrics.display_string_match.total, 0);
    }

    #[test]
    fn rich_displays_count_exact_matches_and_per_format_coverage() {
        let metrics = compare_cells(
            json!([
                formatted_cell(0, json!(1), json!("1.00"), "0.00"),
                formatted_cell(1, json!(2), Value::Null, "0.00"),
                formatted_cell(2, json!(3), json!("wrong"), "0.00")
            ]),
            json!([
                formatted_cell(0, json!(1), json!("1.00"), "0.00"),
                formatted_cell(1, json!(2), json!("2.00"), "0.00"),
                formatted_cell(2, json!(3), json!("3.00"), "0.00")
            ]),
        );

        assert_eq!(metrics.display_string_match.matched, 1);
        assert_eq!(metrics.display_string_match.total, 2);
        assert_eq!(metrics.format_display.len(), 1);
        let format = &metrics.format_display[0];
        assert_eq!(format.code, "0.00");
        assert_eq!(format.oracle_cells, 3);
        assert_eq!(format.wax_display_coverage.covered, 2);
        assert_eq!(format.wax_display_coverage.total, 3);
        assert_eq!(format.display_string_match.matched, 1);
        assert_eq!(format.display_string_match.total, 2);
        assert_eq!(metrics.display_mismatches[0].category, "0.00");
        assert_eq!(metrics.display_mismatches[0].count, 1);
    }

    #[test]
    fn numeric_relative_tolerance_has_a_sharp_edge() {
        let inside = compare_cells(
            json!([cell(
                0,
                "n",
                json!(1_000_000_000.0),
                Value::Null,
                Value::Null
            )]),
            json!([cell(
                0,
                "n",
                json!(1_000_000_000.5),
                Value::Null,
                Value::Null
            )]),
        );
        let outside = compare_cells(
            json!([cell(
                0,
                "n",
                json!(1_000_000_000.0),
                Value::Null,
                Value::Null
            )]),
            json!([cell(
                0,
                "n",
                json!(1_000_002_000.0),
                Value::Null,
                Value::Null
            )]),
        );

        assert_eq!(inside.cell_value_match.matched, 1);
        assert_eq!(outside.cell_value_match.matched, 0);
    }

    #[test]
    fn type_mismatch_does_not_match() {
        let metrics = compare_cells(
            json!([cell(0, "n", json!(1), Value::Null, Value::Null)]),
            json!([cell(0, "s", json!("1"), Value::Null, Value::Null)]),
        );
        assert_eq!(metrics.cell_value_match.matched, 0);
        assert_eq!(metrics.cell_value_match.total, 1);
        assert_eq!(metrics.value_mismatches[0].category, "wax:n / SheetJS:s");
        assert_eq!(metrics.value_mismatches[0].count, 1);
    }

    #[test]
    fn formula_whitespace_is_ignored_but_cached_values_are_not() {
        let metrics = compare_cells(
            json!([cell(0, "n", json!(3), Value::Null, json!("SUM(A1:A2)"))]),
            json!([cell(
                0,
                "n",
                json!(4),
                Value::Null,
                json!(" SUM ( A1:A2 ) ")
            )]),
        );
        assert_eq!(metrics.formula_fidelity.matched, 1);
        assert_eq!(metrics.formula_fidelity.total, 1);
        assert_eq!(metrics.cached_result_fidelity.matched, 0);
        assert_eq!(metrics.cached_result_fidelity.total, 1);
    }

    #[test]
    fn formula_normalization_preserves_quoted_whitespace() {
        let metrics = compare_cells(
            json!([cell(
                0,
                "s",
                json!("a b"),
                Value::Null,
                json!("'My Sheet'!A1 & \"a b\"")
            )]),
            json!([cell(
                0,
                "s",
                json!("ab"),
                Value::Null,
                json!(" 'My Sheet' ! A1 & \"ab\"")
            )]),
        );
        assert_eq!(metrics.formula_fidelity.matched, 0);
        assert_eq!(metrics.formula_fidelity.total, 1);
    }

    #[test]
    fn formula_normalization_preserves_structured_reference_whitespace() {
        let metrics = compare_cells(
            json!([cell(
                0,
                "n",
                json!(1),
                Value::Null,
                json!("SUM(Table1[Column Name])")
            )]),
            json!([cell(
                0,
                "n",
                json!(1),
                Value::Null,
                json!("SUM ( Table1[ColumnName] )")
            )]),
        );
        assert_eq!(metrics.formula_fidelity.matched, 0);
        assert_eq!(metrics.formula_fidelity.total, 1);
    }

    #[test]
    fn truncated_pair_compares_only_emitted_union() {
        let wax = ToolObservation::document(dump(
            Tool::Wax,
            json!([cell(0, "n", json!(1), Value::Null, Value::Null)]),
            true,
            true,
        ));
        let sheetjs = ToolObservation::document(dump(
            Tool::Sheetjs,
            json!([
                cell(0, "n", json!(1), Value::Null, Value::Null),
                cell(1, "n", json!(2), Value::Null, Value::Null)
            ]),
            true,
            true,
        ));
        let metrics = compare("id", "fixture.xlsx", "abc", &wax, &sheetjs);

        assert_eq!(metrics.cell_value_match.matched, 1);
        assert_eq!(metrics.cell_value_match.total, 2);
        assert!(metrics.warnings.is_empty());
    }

    #[test]
    fn one_side_failed_still_records_open_status_without_fake_comparisons() {
        let wax = ToolObservation::document(dump(Tool::Wax, json!([]), false, false));
        let sheetjs = ToolObservation::document(dump(
            Tool::Sheetjs,
            json!([cell(0, "n", json!(1), Value::Null, Value::Null)]),
            false,
            true,
        ));
        let metrics = compare("id", "fixture.xlsx", "abc", &wax, &sheetjs);

        assert!(!metrics.wax.ok);
        assert!(metrics.sheetjs.ok);
        assert_eq!(metrics.cell_value_match.total, 0);
        assert_eq!(metrics.formula_fidelity.total, 0);
    }

    #[test]
    fn wax_open_failure_counts_zero_coverage_for_oracle_format_cells() {
        let wax = ToolObservation::document(dump(Tool::Wax, json!([]), false, false));
        let sheetjs = ToolObservation::document(dump(
            Tool::Sheetjs,
            json!([formatted_cell(0, json!(1), json!("1.00"), "0.00")]),
            false,
            true,
        ));
        let metrics = compare("id", "fixture.xlsx", "abc", &wax, &sheetjs);

        assert_eq!(metrics.format_display.len(), 1);
        assert_eq!(metrics.format_display[0].oracle_cells, 1);
        assert_eq!(metrics.format_display[0].wax_display_coverage.covered, 0);
        assert_eq!(metrics.format_display[0].wax_display_coverage.total, 1);
        assert_eq!(metrics.format_display[0].display_string_match.total, 0);
    }
}

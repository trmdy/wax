use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::aggregate::RatioMetric;
use crate::compare::{CountMetric, CoverageMetric, FileMetrics};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatCoverageReport {
    pub schema: u32,
    pub generated_at: String,
    #[serde(default)]
    pub files_attempted: u64,
    pub ranking: String,
    pub joined_corpus_formats: bool,
    pub totals: FormatCoverageTotals,
    pub formats: Vec<FormatCoverageEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatCoverageTotals {
    pub format_codes: u64,
    pub oracle_cells: u64,
    pub wax_display_coverage: RatioMetric,
    pub display_string_match: RatioMetric,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatCoverageEntry {
    pub code: String,
    pub cell_count: u64,
    pub file_count: u64,
    pub corpus_cell_count: Option<u64>,
    pub corpus_file_count: Option<u64>,
    pub wax_display_coverage: RatioMetric,
    pub display_string_match: RatioMetric,
}

#[derive(Debug, Clone, Default)]
pub struct CorpusFormatIndex {
    formats: BTreeMap<String, CorpusFormatCount>,
}

#[derive(Debug, Clone, Copy)]
struct CorpusFormatCount {
    cell_count: u64,
    file_count: u64,
}

#[derive(Debug, Default)]
struct ObservedFormat {
    cell_count: u64,
    file_count: u64,
    wax_display_coverage: CoverageMetric,
    display_string_match: CountMetric,
}

pub fn aggregate_format_coverage(
    results: &[FileMetrics],
    generated_at: impl Into<String>,
    corpus_formats: Option<&CorpusFormatIndex>,
) -> FormatCoverageReport {
    let mut observed = BTreeMap::<String, ObservedFormat>::new();
    for result in results {
        for metric in &result.format_display {
            let total = observed.entry(metric.code.clone()).or_default();
            total.cell_count += metric.oracle_cells;
            total.file_count += 1;
            total.wax_display_coverage.covered += metric.wax_display_coverage.covered;
            total.wax_display_coverage.total += metric.wax_display_coverage.total;
            total.display_string_match.matched += metric.display_string_match.matched;
            total.display_string_match.total += metric.display_string_match.total;
        }
    }

    let mut total_coverage = CoverageMetric::default();
    let mut total_match = CountMetric::default();
    let mut formats: Vec<_> = observed
        .into_iter()
        .map(|(code, observed)| {
            total_coverage.covered += observed.wax_display_coverage.covered;
            total_coverage.total += observed.wax_display_coverage.total;
            total_match.matched += observed.display_string_match.matched;
            total_match.total += observed.display_string_match.total;
            let corpus = corpus_formats.and_then(|index| index.formats.get(&code));
            FormatCoverageEntry {
                code,
                cell_count: observed.cell_count,
                file_count: observed.file_count,
                corpus_cell_count: corpus.map(|count| count.cell_count),
                corpus_file_count: corpus.map(|count| count.file_count),
                wax_display_coverage: RatioMetric::from_coverage(observed.wax_display_coverage),
                display_string_match: RatioMetric::from_count(observed.display_string_match),
            }
        })
        .collect();

    formats.sort_by(|left, right| {
        let left_count = left.corpus_cell_count.unwrap_or(left.cell_count);
        let right_count = right.corpus_cell_count.unwrap_or(right.cell_count);
        right
            .corpus_cell_count
            .is_some()
            .cmp(&left.corpus_cell_count.is_some())
            .then_with(|| right_count.cmp(&left_count))
            .then_with(|| right.cell_count.cmp(&left.cell_count))
            .then_with(|| left.code.cmp(&right.code))
    });

    FormatCoverageReport {
        schema: 1,
        generated_at: generated_at.into(),
        files_attempted: results.len() as u64,
        ranking: if corpus_formats.is_some() {
            "corpusCellCount".to_owned()
        } else {
            "observedCellCount".to_owned()
        },
        joined_corpus_formats: corpus_formats.is_some(),
        totals: FormatCoverageTotals {
            format_codes: formats.len() as u64,
            oracle_cells: formats.iter().map(|format| format.cell_count).sum(),
            wax_display_coverage: RatioMetric::from_coverage(total_coverage),
            display_string_match: RatioMetric::from_count(total_match),
        },
        formats,
    }
}

pub fn load_corpus_format_index(path: &Path) -> Result<Option<CorpusFormatIndex>> {
    if !path.exists() {
        return Ok(None);
    }

    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid corpus format JSON in {}", path.display()))?;
    let entries = value
        .as_array()
        .or_else(|| value.get("formats").and_then(Value::as_array))
        .or_else(|| value.get("entries").and_then(Value::as_array))
        .with_context(|| {
            format!(
                "{} must be an array or contain a formats array",
                path.display()
            )
        })?;

    let mut formats = BTreeMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let code = entry
            .get("code")
            .and_then(Value::as_str)
            .filter(|code| !code.is_empty())
            .with_context(|| format!("format entry {index} has no non-empty code"))?;
        let cell_count = entry
            .get("cellCount")
            .and_then(Value::as_u64)
            .with_context(|| format!("format entry {index} has no integer cellCount"))?;
        let file_count = entry
            .get("fileCount")
            .and_then(Value::as_u64)
            .with_context(|| format!("format entry {index} has no integer fileCount"))?;
        if formats
            .insert(
                code.to_owned(),
                CorpusFormatCount {
                    cell_count,
                    file_count,
                },
            )
            .is_some()
        {
            bail!("duplicate corpus format code {code:?}");
        }
    }

    Ok(Some(CorpusFormatIndex { formats }))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{aggregate_format_coverage, load_corpus_format_index};
    use crate::compare::{
        CountMetric, CoverageMetric, FileMetrics, FormatDisplayMetric, ToolSummary,
    };

    fn file(formats: Vec<FormatDisplayMetric>) -> FileMetrics {
        let summary = ToolSummary {
            ok: true,
            error: None,
            wall_ms: None,
            peak_rss_bytes: None,
            truncated: false,
        };
        FileMetrics {
            id: "fixture".to_owned(),
            path: "fixture.xlsx".to_owned(),
            sha256: "abc".to_owned(),
            ext: "xlsx".to_owned(),
            private: false,
            wax: summary.clone(),
            sheetjs: summary,
            cell_value_match: CountMetric::default(),
            wax_display_coverage: CoverageMetric::default(),
            sheetjs_display_coverage: CoverageMetric::default(),
            display_string_match: CountMetric::default(),
            formula_fidelity: CountMetric::default(),
            cached_result_fidelity: CountMetric::default(),
            format_display: formats,
            value_mismatches: Vec::new(),
            display_mismatches: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn format(
        code: &str,
        cells: u64,
        covered: u64,
        compared: u64,
        matched: u64,
    ) -> FormatDisplayMetric {
        FormatDisplayMetric {
            code: code.to_owned(),
            oracle_cells: cells,
            wax_display_coverage: CoverageMetric {
                covered,
                total: cells,
            },
            display_string_match: CountMetric {
                matched,
                total: compared,
            },
        }
    }

    #[test]
    fn aggregates_and_ranks_observed_format_metrics() {
        let results = vec![
            file(vec![format("0.00", 10, 8, 8, 7), format("0%", 3, 1, 1, 0)]),
            file(vec![format("0%", 4, 4, 4, 4)]),
        ];

        let report = aggregate_format_coverage(&results, "2026-07-28T00:00:00Z", None);

        assert_eq!(report.ranking, "observedCellCount");
        assert!(!report.joined_corpus_formats);
        assert_eq!(report.files_attempted, 2);
        assert_eq!(report.formats[0].code, "0.00");
        assert_eq!(report.formats[0].file_count, 1);
        assert_eq!(report.formats[1].code, "0%");
        assert_eq!(report.formats[1].cell_count, 7);
        assert_eq!(report.formats[1].file_count, 2);
        assert_eq!(report.totals.oracle_cells, 17);
        assert_eq!(report.totals.wax_display_coverage.matched, 13);
        assert_eq!(report.totals.wax_display_coverage.total, 17);
        assert_eq!(report.totals.display_string_match.matched, 11);
        assert_eq!(report.totals.display_string_match.total, 13);
    }

    #[test]
    fn corpus_join_annotates_and_controls_ranking() {
        let root = tempdir().unwrap();
        let path = root.path().join("corpus-formats.json");
        fs::write(
            &path,
            r#"{
              "formats": [
                {"code": "0%", "cellCount": 1000, "fileCount": 20},
                {"code": "0.00", "cellCount": 50, "fileCount": 5}
              ]
            }"#,
        )
        .unwrap();
        let index = load_corpus_format_index(&path).unwrap().unwrap();
        let results = vec![file(vec![
            format("0.00", 100, 100, 100, 100),
            format("0%", 1, 1, 1, 1),
        ])];

        let report = aggregate_format_coverage(&results, "2026-07-28T00:00:00Z", Some(&index));

        assert_eq!(report.ranking, "corpusCellCount");
        assert!(report.joined_corpus_formats);
        assert_eq!(report.formats[0].code, "0%");
        assert_eq!(report.formats[0].corpus_cell_count, Some(1000));
        assert_eq!(report.formats[0].corpus_file_count, Some(20));
    }

    #[test]
    fn absent_corpus_format_file_degrades_to_no_join() {
        let root = tempdir().unwrap();
        assert!(load_corpus_format_index(&root.path().join("missing.json"))
            .unwrap()
            .is_none());
    }
}

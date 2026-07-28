use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::compare::{CountMetric, CoverageMetric, FileMetrics};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scoreboard {
    pub schema: u32,
    pub generated_at: String,
    pub files_attempted: u64,
    pub files_skipped: u64,
    pub metrics: AggregateMetrics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateMetrics {
    pub files_opened: OpenedMetrics,
    pub cell_value_match: RatioMetric,
    pub display_string_coverage: ToolCoverageMetrics,
    #[serde(default)]
    pub display_string_match: RatioMetric,
    pub formula_fidelity: RatioMetric,
    pub cached_result_fidelity: RatioMetric,
    pub parse_time_ms: ToolPercentileMetrics,
    pub peak_rss_bytes: ToolRssMetrics,
    pub window_latency_ms: ToolNullableMetrics,
    #[serde(default)]
    pub per_extension: BTreeMap<String, ExtensionMetrics>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpenedMetrics {
    pub wax: RatioMetric,
    pub sheetjs: RatioMetric,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolCoverageMetrics {
    pub wax: RatioMetric,
    pub sheetjs: RatioMetric,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RatioMetric {
    pub matched: u64,
    pub total: u64,
    pub percent: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionMetrics {
    pub files_attempted: u64,
    pub files_opened: OpenedMetrics,
    pub cell_value_match: RatioMetric,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolPercentileMetrics {
    pub wax: PercentileMetric,
    pub sheetjs: PercentileMetric,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PercentileMetric {
    pub p50: Option<u64>,
    pub p95: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolRssMetrics {
    pub wax: RssMetric,
    pub sheetjs: RssMetric,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RssMetric {
    pub p50: Option<u64>,
    pub max: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolNullableMetrics {
    pub wax: Option<u64>,
    pub sheetjs: Option<u64>,
}

pub fn aggregate(
    results: &[FileMetrics],
    files_skipped: u64,
    generated_at: impl Into<String>,
) -> Scoreboard {
    let attempted = results.len() as u64;
    let wax_opened = results.iter().filter(|result| result.wax.ok).count() as u64;
    let sheetjs_opened = results.iter().filter(|result| result.sheetjs.ok).count() as u64;

    let cell_value_match = sum_counts(results.iter().map(|result| result.cell_value_match));
    let display_string_match = sum_counts(results.iter().map(|result| result.display_string_match));
    let formula_fidelity = sum_counts(results.iter().map(|result| result.formula_fidelity));
    let cached_result_fidelity =
        sum_counts(results.iter().map(|result| result.cached_result_fidelity));
    let wax_display = sum_coverage(results.iter().map(|result| result.wax_display_coverage));
    let sheetjs_display =
        sum_coverage(results.iter().map(|result| result.sheetjs_display_coverage));

    let wax_wall_times: Vec<_> = results
        .iter()
        .filter_map(|result| result.wax.wall_ms)
        .map(|ms| ms.round() as u64)
        .collect();
    let sheetjs_wall_times: Vec<_> = results
        .iter()
        .filter_map(|result| result.sheetjs.wall_ms)
        .map(|ms| ms.round() as u64)
        .collect();
    let wax_rss: Vec<_> = results
        .iter()
        .filter_map(|result| result.wax.peak_rss_bytes)
        .collect();
    let sheetjs_rss: Vec<_> = results
        .iter()
        .filter_map(|result| result.sheetjs.peak_rss_bytes)
        .collect();

    Scoreboard {
        schema: 1,
        generated_at: generated_at.into(),
        files_attempted: attempted,
        files_skipped,
        metrics: AggregateMetrics {
            files_opened: OpenedMetrics {
                wax: RatioMetric::new(wax_opened, attempted),
                sheetjs: RatioMetric::new(sheetjs_opened, attempted),
            },
            cell_value_match: RatioMetric::from_count(cell_value_match),
            display_string_coverage: ToolCoverageMetrics {
                wax: RatioMetric::from_coverage(wax_display),
                sheetjs: RatioMetric::from_coverage(sheetjs_display),
            },
            display_string_match: RatioMetric::from_count(display_string_match),
            formula_fidelity: RatioMetric::from_count(formula_fidelity),
            cached_result_fidelity: RatioMetric::from_count(cached_result_fidelity),
            parse_time_ms: ToolPercentileMetrics {
                wax: percentiles(&wax_wall_times),
                sheetjs: percentiles(&sheetjs_wall_times),
            },
            peak_rss_bytes: ToolRssMetrics {
                wax: rss_metrics(&wax_rss),
                sheetjs: rss_metrics(&sheetjs_rss),
            },
            window_latency_ms: ToolNullableMetrics {
                wax: None,
                sheetjs: None,
            },
            per_extension: per_extension(results),
        },
    }
}

impl RatioMetric {
    pub(crate) fn new(matched: u64, total: u64) -> Self {
        Self {
            matched,
            total,
            percent: (total != 0).then_some(matched as f64 * 100.0 / total as f64),
        }
    }

    pub(crate) fn from_count(metric: CountMetric) -> Self {
        Self::new(metric.matched, metric.total)
    }

    pub(crate) fn from_coverage(metric: CoverageMetric) -> Self {
        Self::new(metric.covered, metric.total)
    }
}

fn per_extension(results: &[FileMetrics]) -> BTreeMap<String, ExtensionMetrics> {
    let mut grouped = BTreeMap::<String, Vec<&FileMetrics>>::new();
    for result in results {
        let extension = if result.ext.is_empty() {
            "unknown".to_owned()
        } else {
            result.ext.to_ascii_lowercase()
        };
        grouped.entry(extension).or_default().push(result);
    }

    grouped
        .into_iter()
        .map(|(extension, results)| {
            let attempted = results.len() as u64;
            let wax_opened = results.iter().filter(|result| result.wax.ok).count() as u64;
            let sheetjs_opened = results.iter().filter(|result| result.sheetjs.ok).count() as u64;
            let cell_value_match = sum_counts(results.iter().map(|result| result.cell_value_match));
            (
                extension,
                ExtensionMetrics {
                    files_attempted: attempted,
                    files_opened: OpenedMetrics {
                        wax: RatioMetric::new(wax_opened, attempted),
                        sheetjs: RatioMetric::new(sheetjs_opened, attempted),
                    },
                    cell_value_match: RatioMetric::from_count(cell_value_match),
                },
            )
        })
        .collect()
}

fn sum_counts(metrics: impl Iterator<Item = CountMetric>) -> CountMetric {
    metrics.fold(CountMetric::default(), |mut total, metric| {
        total.matched += metric.matched;
        total.total += metric.total;
        total
    })
}

fn sum_coverage(metrics: impl Iterator<Item = CoverageMetric>) -> CoverageMetric {
    metrics.fold(CoverageMetric::default(), |mut total, metric| {
        total.covered += metric.covered;
        total.total += metric.total;
        total
    })
}

fn percentiles(values: &[u64]) -> PercentileMetric {
    PercentileMetric {
        p50: nearest_rank(values, 50),
        p95: nearest_rank(values, 95),
    }
}

fn rss_metrics(values: &[u64]) -> RssMetric {
    RssMetric {
        p50: nearest_rank(values, 50),
        max: values.iter().copied().max(),
    }
}

fn nearest_rank(values: &[u64], percentile: usize) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort_unstable();
    let rank = (percentile * values.len()).div_ceil(100);
    Some(values[rank.saturating_sub(1)])
}

#[cfg(test)]
mod tests {
    use super::{aggregate, nearest_rank};
    use crate::compare::{CountMetric, CoverageMetric, FileMetrics, ToolSummary};

    fn file(
        ext: &str,
        wax_ok: bool,
        sheetjs_ok: bool,
        cell_value_match: CountMetric,
        display_string_match: CountMetric,
    ) -> FileMetrics {
        let summary = |ok| ToolSummary {
            ok,
            error: None,
            wall_ms: None,
            peak_rss_bytes: None,
            truncated: false,
        };
        FileMetrics {
            id: format!("fixture.{ext}"),
            path: format!("fixture.{ext}"),
            sha256: "abc".to_owned(),
            ext: ext.to_owned(),
            private: false,
            wax: summary(wax_ok),
            sheetjs: summary(sheetjs_ok),
            cell_value_match,
            wax_display_coverage: CoverageMetric::default(),
            sheetjs_display_coverage: CoverageMetric::default(),
            display_string_match,
            formula_fidelity: CountMetric::default(),
            cached_result_fidelity: CountMetric::default(),
            format_display: Vec::new(),
            value_mismatches: Vec::new(),
            display_mismatches: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn nearest_rank_percentiles_are_deterministic() {
        let values = [40, 10, 30, 20];
        assert_eq!(nearest_rank(&values, 50), Some(20));
        assert_eq!(nearest_rank(&values, 95), Some(40));
        assert_eq!(nearest_rank(&[], 50), None);
    }

    #[test]
    fn empty_corpus_has_null_metrics_instead_of_dividing_by_zero() {
        let scoreboard = aggregate(&[], 2, "2026-07-28T00:00:00Z");

        assert_eq!(scoreboard.files_attempted, 0);
        assert_eq!(scoreboard.metrics.files_opened.wax.percent, None);
        assert_eq!(scoreboard.metrics.cell_value_match.percent, None);
        assert_eq!(scoreboard.metrics.display_string_match.percent, None);
        assert_eq!(scoreboard.metrics.parse_time_ms.wax.p50, None);
        assert_eq!(scoreboard.metrics.peak_rss_bytes.sheetjs.max, None);
        assert_eq!(scoreboard.metrics.window_latency_ms.wax, None);
        assert!(scoreboard.metrics.per_extension.is_empty());
    }

    #[test]
    fn aggregates_display_match_and_manifest_extensions() {
        let results = vec![
            file(
                "xlsx",
                true,
                true,
                CountMetric {
                    matched: 8,
                    total: 10,
                },
                CountMetric {
                    matched: 4,
                    total: 5,
                },
            ),
            file(
                "XLSX",
                false,
                true,
                CountMetric::default(),
                CountMetric::default(),
            ),
            file(
                "ods",
                true,
                true,
                CountMetric {
                    matched: 1,
                    total: 2,
                },
                CountMetric {
                    matched: 1,
                    total: 2,
                },
            ),
        ];

        let scoreboard = aggregate(&results, 0, "2026-07-28T00:00:00Z");

        assert_eq!(scoreboard.metrics.display_string_match.matched, 5);
        assert_eq!(scoreboard.metrics.display_string_match.total, 7);
        let xlsx = &scoreboard.metrics.per_extension["xlsx"];
        assert_eq!(xlsx.files_attempted, 2);
        assert_eq!(xlsx.files_opened.wax.matched, 1);
        assert_eq!(xlsx.files_opened.sheetjs.matched, 2);
        assert_eq!(xlsx.cell_value_match.matched, 8);
        assert_eq!(xlsx.cell_value_match.total, 10);
        assert_eq!(scoreboard.metrics.per_extension["ods"].files_attempted, 1);
    }
}

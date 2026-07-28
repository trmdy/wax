use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::compare::{CountMetric, CoverageMetric, FileMetrics};
use crate::roundtrip::SofficeAvailability;
use crate::serve::ServeAvailability;

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
    pub open_via_serve: Option<RatioMetric>,
    #[serde(default)]
    pub window_latency_percentiles_ms: FloatPercentileMetric,
    #[serde(default)]
    pub serve_peak_rss_bytes: RssMetric,
    #[serde(default)]
    pub serve_status: ServeStatusMetric,
    #[serde(default)]
    pub per_extension: BTreeMap<String, ExtensionMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_trip: Option<RoundTripMetrics>,
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
    #[serde(default)]
    pub formula_fidelity: RatioMetric,
    #[serde(default)]
    pub cached_result_fidelity: RatioMetric,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FloatPercentileMetric {
    pub p50: Option<f64>,
    pub p95: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolRssMetrics {
    pub wax: RssMetric,
    pub sheetjs: RssMetric,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RssMetric {
    pub p50: Option<u64>,
    pub max: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolNullableMetrics {
    pub wax: Option<u64>,
    pub sheetjs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServeStatusMetric {
    pub status: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundTripMetrics {
    pub files_clean: RatioMetric,
    pub value_match: RatioMetric,
    pub display_match: RatioMetric,
    pub oracle_open_rate: RatioMetric,
    pub soffice_open_rate: RatioMetric,
    pub skipped_truncated: u64,
    pub status: RoundTripStatusMetric,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundTripStatusMetric {
    pub status: String,
    pub reason: Option<String>,
    pub soffice_status: String,
    pub soffice_reason: Option<String>,
}

impl Default for RoundTripStatusMetric {
    fn default() -> Self {
        Self {
            status: "available".to_owned(),
            reason: None,
            soffice_status: "disabled".to_owned(),
            soffice_reason: Some("soffice disabled".to_owned()),
        }
    }
}

impl Default for ServeStatusMetric {
    fn default() -> Self {
        Self {
            status: "unavailable".to_owned(),
            reason: None,
        }
    }
}

pub fn aggregate(
    results: &[FileMetrics],
    files_skipped: u64,
    generated_at: impl Into<String>,
) -> Scoreboard {
    let availability = if results.iter().any(|result| result.serve.is_some()) {
        ServeAvailability::Available
    } else {
        ServeAvailability::Unavailable {
            reason: "serve metrics were not collected".to_owned(),
        }
    };
    aggregate_with_serve(results, files_skipped, generated_at, &availability)
}

pub fn aggregate_with_serve(
    results: &[FileMetrics],
    files_skipped: u64,
    generated_at: impl Into<String>,
    serve_availability: &ServeAvailability,
) -> Scoreboard {
    aggregate_with_serve_and_round_trip(
        results,
        files_skipped,
        generated_at,
        serve_availability,
        &SofficeAvailability::Disabled,
    )
}

pub fn aggregate_with_serve_and_round_trip(
    results: &[FileMetrics],
    files_skipped: u64,
    generated_at: impl Into<String>,
    serve_availability: &ServeAvailability,
    soffice_availability: &SofficeAvailability,
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
    let serve_window_latencies: Vec<_> = results
        .iter()
        .filter_map(|result| result.serve.as_ref())
        .flat_map(|serve| &serve.requests)
        .filter(|request| request.op == "window")
        .map(|request| request.wall_ms)
        .collect();
    let serve_rss: Vec<_> = results
        .iter()
        .filter_map(|result| result.serve.as_ref())
        .filter_map(|serve| serve.peak_rss_bytes)
        .collect();
    let serve_opened = results
        .iter()
        .filter_map(|result| result.serve.as_ref())
        .filter(|serve| serve.open_ok)
        .count() as u64;
    let serve_was_available = serve_availability.is_available();

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
            open_via_serve: serve_was_available.then(|| RatioMetric::new(serve_opened, attempted)),
            window_latency_percentiles_ms: if serve_was_available {
                float_percentiles(&serve_window_latencies)
            } else {
                FloatPercentileMetric::default()
            },
            serve_peak_rss_bytes: if serve_was_available {
                rss_metrics(&serve_rss)
            } else {
                RssMetric {
                    p50: None,
                    max: None,
                }
            },
            serve_status: ServeStatusMetric {
                status: serve_availability.status().to_owned(),
                reason: serve_availability.reason().map(str::to_owned),
            },
            per_extension: per_extension(results),
            round_trip: Some(aggregate_round_trip(results, soffice_availability)),
        },
    }
}

fn aggregate_round_trip(
    results: &[FileMetrics],
    soffice_availability: &SofficeAvailability,
) -> RoundTripMetrics {
    let round_trips: Vec<_> = results
        .iter()
        .filter_map(|result| result.round_trip.as_ref())
        .collect();
    let attempted: Vec<_> = round_trips
        .iter()
        .copied()
        .filter(|round_trip| round_trip.was_attempted())
        .collect();
    let skipped_truncated = round_trips
        .iter()
        .filter(|round_trip| round_trip.status == "skippedTruncated")
        .count() as u64;
    let unavailable = attempted
        .iter()
        .filter(|round_trip| round_trip.is_unavailable())
        .count();
    let has_available_evidence = attempted
        .iter()
        .any(|round_trip| !round_trip.is_unavailable());
    let export_unavailable = unavailable != 0 && !has_available_evidence;

    let status = RoundTripStatusMetric {
        status: if export_unavailable {
            "unavailable".to_owned()
        } else {
            "available".to_owned()
        },
        reason: export_unavailable.then(|| "xlsx export unavailable".to_owned()),
        soffice_status: soffice_availability.status().to_owned(),
        soffice_reason: soffice_availability.reason().map(str::to_owned),
    };
    if export_unavailable {
        return RoundTripMetrics {
            files_clean: RatioMetric {
                matched: 0,
                total: attempted.len() as u64,
                percent: None,
            },
            skipped_truncated,
            status,
            ..RoundTripMetrics::default()
        };
    }

    let value_match = sum_counts(attempted.iter().map(|round_trip| round_trip.value_match));
    let display_match = sum_counts(attempted.iter().map(|round_trip| round_trip.display_match));
    let oracle_total = attempted
        .iter()
        .filter(|round_trip| round_trip.oracle_open.is_some())
        .count() as u64;
    let oracle_opened = attempted
        .iter()
        .filter(|round_trip| round_trip.oracle_open == Some(true))
        .count() as u64;
    let soffice_total = attempted
        .iter()
        .filter(|round_trip| round_trip.soffice_open.is_some())
        .count() as u64;
    let soffice_opened = attempted
        .iter()
        .filter(|round_trip| round_trip.soffice_open == Some(true))
        .count() as u64;

    RoundTripMetrics {
        files_clean: RatioMetric::new(
            attempted
                .iter()
                .filter(|round_trip| round_trip.is_clean())
                .count() as u64,
            attempted.len() as u64,
        ),
        value_match: RatioMetric::from_count(value_match),
        display_match: RatioMetric::from_count(display_match),
        oracle_open_rate: RatioMetric::new(oracle_opened, oracle_total),
        soffice_open_rate: RatioMetric::new(soffice_opened, soffice_total),
        skipped_truncated,
        status,
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
            let formula_fidelity = sum_counts(results.iter().map(|result| result.formula_fidelity));
            let cached_result_fidelity =
                sum_counts(results.iter().map(|result| result.cached_result_fidelity));
            (
                extension,
                ExtensionMetrics {
                    files_attempted: attempted,
                    files_opened: OpenedMetrics {
                        wax: RatioMetric::new(wax_opened, attempted),
                        sheetjs: RatioMetric::new(sheetjs_opened, attempted),
                    },
                    cell_value_match: RatioMetric::from_count(cell_value_match),
                    formula_fidelity: RatioMetric::from_count(formula_fidelity),
                    cached_result_fidelity: RatioMetric::from_count(cached_result_fidelity),
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

fn float_percentiles(values: &[f64]) -> FloatPercentileMetric {
    FloatPercentileMetric {
        p50: nearest_rank_float(values, 50),
        p95: nearest_rank_float(values, 95),
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

fn nearest_rank_float(values: &[f64], percentile: usize) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let rank = (percentile * values.len()).div_ceil(100);
    Some(values[rank.saturating_sub(1)])
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate, aggregate_with_serve, aggregate_with_serve_and_round_trip, nearest_rank,
        nearest_rank_float,
    };
    use crate::compare::{CountMetric, CoverageMetric, FileMetrics, ToolSummary};
    use crate::roundtrip::{RoundTripFailure, RoundTripFileMetrics, SofficeAvailability};
    use crate::serve::{ServeAvailability, ServeFileMetrics, ServeRequestMetric};

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
            serve: None,
            round_trip: None,
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
        let float_values = [4.5, 1.25, 3.75, 2.5];
        assert_eq!(nearest_rank_float(&float_values, 50), Some(2.5));
        assert_eq!(nearest_rank_float(&float_values, 95), Some(4.5));
        assert_eq!(nearest_rank_float(&[], 50), None);
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
        let mut results = vec![
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
        results[0].formula_fidelity = CountMetric {
            matched: 99,
            total: 100,
        };
        results[0].cached_result_fidelity = CountMetric {
            matched: 80,
            total: 100,
        };

        let scoreboard = aggregate(&results, 0, "2026-07-28T00:00:00Z");

        assert_eq!(scoreboard.metrics.display_string_match.matched, 5);
        assert_eq!(scoreboard.metrics.display_string_match.total, 7);
        let xlsx = &scoreboard.metrics.per_extension["xlsx"];
        assert_eq!(xlsx.files_attempted, 2);
        assert_eq!(xlsx.files_opened.wax.matched, 1);
        assert_eq!(xlsx.files_opened.sheetjs.matched, 2);
        assert_eq!(xlsx.cell_value_match.matched, 8);
        assert_eq!(xlsx.cell_value_match.total, 10);
        assert_eq!(xlsx.formula_fidelity.matched, 99);
        assert_eq!(xlsx.formula_fidelity.total, 100);
        assert_eq!(xlsx.cached_result_fidelity.matched, 80);
        assert_eq!(xlsx.cached_result_fidelity.total, 100);
        assert_eq!(scoreboard.metrics.per_extension["ods"].files_attempted, 1);
    }

    #[test]
    fn aggregates_serve_opens_window_latencies_and_rss() {
        let mut results = vec![
            file(
                "xlsx",
                true,
                true,
                CountMetric::default(),
                CountMetric::default(),
            ),
            file(
                "xlsx",
                true,
                true,
                CountMetric::default(),
                CountMetric::default(),
            ),
        ];
        results[0].serve = Some(ServeFileMetrics {
            open_ok: true,
            requests: vec![
                request(1, "version", 50.0),
                request(2, "window", 4.5),
                request(3, "window", 1.25),
            ],
            peak_rss_bytes: Some(100),
            ..ServeFileMetrics::default()
        });
        results[1].serve = Some(ServeFileMetrics {
            open_ok: false,
            requests: vec![request(1, "window", 3.75), request(2, "window", 2.5)],
            peak_rss_bytes: Some(200),
            ..ServeFileMetrics::default()
        });

        let scoreboard = aggregate_with_serve(
            &results,
            0,
            "2026-07-28T00:00:00Z",
            &ServeAvailability::Available,
        );

        assert_eq!(
            scoreboard.metrics.open_via_serve.as_ref().unwrap().matched,
            1
        );
        assert_eq!(scoreboard.metrics.open_via_serve.as_ref().unwrap().total, 2);
        assert_eq!(
            scoreboard.metrics.window_latency_percentiles_ms.p50,
            Some(2.5)
        );
        assert_eq!(
            scoreboard.metrics.window_latency_percentiles_ms.p95,
            Some(4.5)
        );
        assert_eq!(scoreboard.metrics.serve_peak_rss_bytes.p50, Some(100));
        assert_eq!(scoreboard.metrics.serve_peak_rss_bytes.max, Some(200));
    }

    #[test]
    fn round_trip_bookkeeping_counts_export_failure_and_truncated_skip_honestly() {
        let mut results = vec![
            file(
                "xlsx",
                true,
                true,
                CountMetric::default(),
                CountMetric::default(),
            ),
            file(
                "xls",
                true,
                true,
                CountMetric::default(),
                CountMetric::default(),
            ),
            file(
                "ods",
                true,
                true,
                CountMetric::default(),
                CountMetric::default(),
            ),
        ];
        results[0].round_trip = Some(RoundTripFileMetrics {
            status: "clean".to_owned(),
            value_match: CountMetric {
                matched: 9,
                total: 10,
            },
            display_match: CountMetric {
                matched: 8,
                total: 10,
            },
            oracle_open: Some(true),
            ..RoundTripFileMetrics::default()
        });
        results[1].round_trip = Some(RoundTripFileMetrics {
            status: "failed".to_owned(),
            error: Some(RoundTripFailure {
                stage: "export".to_owned(),
                code: "bad_request".to_owned(),
                msg: "fixture failure".to_owned(),
            }),
            ..RoundTripFileMetrics::default()
        });
        results[2].round_trip = Some(RoundTripFileMetrics::skipped_truncated());

        let scoreboard = aggregate_with_serve_and_round_trip(
            &results,
            0,
            "2026-07-28T00:00:00Z",
            &ServeAvailability::Disabled,
            &SofficeAvailability::Unavailable {
                reason: "soffice unavailable".to_owned(),
            },
        );
        let round_trip = scoreboard.metrics.round_trip.unwrap();

        assert_eq!(round_trip.files_clean.matched, 1);
        assert_eq!(round_trip.files_clean.total, 2);
        assert_eq!(round_trip.value_match.matched, 9);
        assert_eq!(round_trip.value_match.total, 10);
        assert_eq!(round_trip.display_match.matched, 8);
        assert_eq!(round_trip.display_match.total, 10);
        assert_eq!(round_trip.oracle_open_rate.matched, 1);
        assert_eq!(round_trip.oracle_open_rate.total, 1);
        assert_eq!(round_trip.skipped_truncated, 1);
        assert_eq!(round_trip.status.soffice_status, "unavailable");
    }

    #[test]
    fn all_unavailable_exports_produce_null_rates_instead_of_fake_zeroes() {
        let mut results = vec![file(
            "xlsx",
            true,
            true,
            CountMetric::default(),
            CountMetric::default(),
        )];
        results[0].round_trip = Some(RoundTripFileMetrics {
            status: "xlsxExportUnavailable".to_owned(),
            error: Some(RoundTripFailure {
                stage: "export".to_owned(),
                code: "internal".to_owned(),
                msg: "writer stub".to_owned(),
            }),
            ..RoundTripFileMetrics::default()
        });

        let scoreboard = aggregate(&results, 0, "2026-07-28T00:00:00Z");
        let round_trip = scoreboard.metrics.round_trip.unwrap();

        assert_eq!(round_trip.status.status, "unavailable");
        assert_eq!(
            round_trip.status.reason.as_deref(),
            Some("xlsx export unavailable")
        );
        assert_eq!(round_trip.files_clean.total, 1);
        assert_eq!(round_trip.files_clean.percent, None);
        assert_eq!(round_trip.value_match.percent, None);
        assert_eq!(round_trip.oracle_open_rate.percent, None);
    }

    fn request(id: u64, op: &str, wall_ms: f64) -> ServeRequestMetric {
        ServeRequestMetric {
            id,
            op: op.to_owned(),
            wall_ms,
            ok: true,
            error: None,
        }
    }
}

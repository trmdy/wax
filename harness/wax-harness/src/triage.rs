use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::compare::{FileMetrics, MismatchBucket};

const MAX_CATEGORIES: usize = 20;
const MAX_EXAMPLES: usize = 5;

#[derive(Debug, Default)]
struct CategoryAggregate {
    count: u64,
    examples: BTreeSet<String>,
}

pub fn render_triage(results: &[FileMetrics], generated_at: &str) -> String {
    let mut open_failures = BTreeMap::<String, CategoryAggregate>::new();
    let mut value_mismatches = BTreeMap::<String, CategoryAggregate>::new();
    let mut display_mismatches = BTreeMap::<String, CategoryAggregate>::new();

    for result in results {
        if !result.wax.ok {
            let category = result
                .wax
                .error
                .as_ref()
                .map(|error| error.code.as_str())
                .unwrap_or("unknown");
            record(&mut open_failures, category, 1, example_path(result));
        }
        record_buckets(&mut value_mismatches, &result.value_mismatches, result);
        record_buckets(&mut display_mismatches, &result.display_mismatches, result);
    }

    let mut output = String::new();
    writeln!(output, "# wax mismatch triage\n").unwrap();
    writeln!(output, "Generated: `{generated_at}`\n").unwrap();
    writeln!(output, "Files compared: {}.\n", results.len()).unwrap();
    writeln!(
        output,
        "Counts include private corpus files; example paths deliberately omit them."
    )
    .unwrap();
    render_section(
        &mut output,
        "Wax open failures by error code",
        open_failures,
    );
    render_section(
        &mut output,
        "Value mismatches by type pair",
        value_mismatches,
    );
    render_section(
        &mut output,
        "Display mismatches by format code",
        display_mismatches,
    );
    render_round_trip_merge_defects(&mut output, results);
    render_round_trip_export_drops(&mut output, results);
    render_round_trip_failures(&mut output, results);
    render_oracle_read_back_defects(&mut output, results);
    output
}

fn render_round_trip_merge_defects(output: &mut String, results: &[FileMetrics]) {
    writeln!(output, "\n## Round-trip merge defects\n").unwrap();
    let private_count = results
        .iter()
        .filter(|result| {
            result.private
                && result
                    .round_trip
                    .as_ref()
                    .is_some_and(|round_trip| !round_trip.merge_defects.is_empty())
        })
        .count();
    let public: Vec<_> = results
        .iter()
        .filter(|result| {
            !result.private
                && result
                    .round_trip
                    .as_ref()
                    .is_some_and(|round_trip| !round_trip.merge_defects.is_empty())
        })
        .collect();
    if public.is_empty() && private_count == 0 {
        writeln!(output, "No disagreements observed.").unwrap();
        return;
    }
    if private_count != 0 {
        writeln!(
            output,
            "{private_count} private file(s) had merge defects; paths are omitted.\n"
        )
        .unwrap();
    }
    if public.is_empty() {
        return;
    }
    writeln!(output, "| File | Defect |").unwrap();
    writeln!(output, "| --- | --- |").unwrap();
    for result in public {
        let round_trip = result
            .round_trip
            .as_ref()
            .expect("filtered result must have round-trip metrics");
        for defect in &round_trip.merge_defects {
            writeln!(
                output,
                "| {} | {} |",
                inline_code(&result.path),
                inline_code(defect)
            )
            .unwrap();
        }
    }
}

fn render_oracle_read_back_defects(output: &mut String, results: &[FileMetrics]) {
    writeln!(
        output,
        "\n## Oracle read-back failures on wax-clean exports\n"
    )
    .unwrap();
    let private_count = results
        .iter()
        .filter(|result| {
            result.private
                && result.round_trip.as_ref().is_some_and(|round_trip| {
                    round_trip.is_clean() && round_trip.oracle_open == Some(false)
                })
        })
        .count();
    let defects: Vec<_> = results
        .iter()
        .filter(|result| {
            !result.private
                && result.round_trip.as_ref().is_some_and(|round_trip| {
                    round_trip.is_clean() && round_trip.oracle_open == Some(false)
                })
        })
        .collect();
    if defects.is_empty() && private_count == 0 {
        writeln!(output, "No disagreements observed.").unwrap();
        return;
    }
    if private_count != 0 {
        writeln!(
            output,
            "{private_count} private file(s) had oracle read-back failures; paths are omitted.\n"
        )
        .unwrap();
    }
    if defects.is_empty() {
        return;
    }
    writeln!(
        output,
        "These are interoperability defects, not automatic losses against the wax model. Record adjudication evidence in `harness/adjudications.md`.\n"
    )
    .unwrap();
    writeln!(output, "| File | Oracle error |").unwrap();
    writeln!(output, "| --- | --- |").unwrap();
    for result in defects {
        let round_trip = result
            .round_trip
            .as_ref()
            .expect("filtered result must have round-trip metrics");
        let error = round_trip
            .oracle_error
            .as_ref()
            .map(|error| format!("{}: {}", error.code, error.msg))
            .unwrap_or_else(|| "unknown oracle failure".to_owned());
        writeln!(
            output,
            "| {} | {} |",
            inline_code(&result.path),
            inline_code(&error)
        )
        .unwrap();
    }
}

fn render_round_trip_export_drops(output: &mut String, results: &[FileMetrics]) {
    writeln!(output, "\n## Round-trip export drops\n").unwrap();
    let private_count = results
        .iter()
        .filter(|result| {
            result.private
                && result
                    .round_trip
                    .as_ref()
                    .is_some_and(|round_trip| !round_trip.export_dropped.is_empty())
        })
        .count();
    let public: Vec<_> = results
        .iter()
        .filter(|result| {
            !result.private
                && result
                    .round_trip
                    .as_ref()
                    .is_some_and(|round_trip| !round_trip.export_dropped.is_empty())
        })
        .collect();
    if public.is_empty() && private_count == 0 {
        writeln!(output, "No disagreements observed.").unwrap();
        return;
    }
    if private_count != 0 {
        writeln!(
            output,
            "{private_count} private file(s) had export drops; paths are omitted.\n"
        )
        .unwrap();
    }
    if public.is_empty() {
        return;
    }
    writeln!(output, "| File | Dropped during export |").unwrap();
    writeln!(output, "| --- | --- |").unwrap();
    for result in public {
        let round_trip = result
            .round_trip
            .as_ref()
            .expect("filtered result must have round-trip metrics");
        for dropped in &round_trip.export_dropped {
            writeln!(
                output,
                "| {} | {} |",
                inline_code(&result.path),
                inline_code(dropped)
            )
            .unwrap();
        }
    }
}

fn render_round_trip_failures(output: &mut String, results: &[FileMetrics]) {
    writeln!(output, "\n## Round-trip failures\n").unwrap();
    let private_count = results
        .iter()
        .filter(|result| {
            result.private
                && result
                    .round_trip
                    .as_ref()
                    .is_some_and(|round_trip| round_trip.error.is_some())
        })
        .count();
    let public: Vec<_> = results
        .iter()
        .filter(|result| {
            !result.private
                && result
                    .round_trip
                    .as_ref()
                    .is_some_and(|round_trip| round_trip.error.is_some())
        })
        .collect();
    if public.is_empty() && private_count == 0 {
        writeln!(output, "No disagreements observed.").unwrap();
        return;
    }
    if private_count != 0 {
        writeln!(
            output,
            "{private_count} private file(s) had round-trip failures; paths are omitted.\n"
        )
        .unwrap();
    }
    if public.is_empty() {
        return;
    }
    writeln!(output, "| File | Failure |").unwrap();
    writeln!(output, "| --- | --- |").unwrap();
    for result in public {
        let failure = result
            .round_trip
            .as_ref()
            .and_then(|round_trip| round_trip.error.as_ref())
            .expect("filtered result must have a round-trip failure");
        let detail = format!("{}: {}: {}", failure.stage, failure.code, failure.msg);
        writeln!(
            output,
            "| {} | {} |",
            inline_code(&result.path),
            inline_code(&detail)
        )
        .unwrap();
    }
}

fn record_buckets(
    totals: &mut BTreeMap<String, CategoryAggregate>,
    buckets: &[MismatchBucket],
    result: &FileMetrics,
) {
    for bucket in buckets {
        record(totals, &bucket.category, bucket.count, example_path(result));
    }
}

fn record(
    totals: &mut BTreeMap<String, CategoryAggregate>,
    category: &str,
    count: u64,
    example: Option<&str>,
) {
    let total = totals.entry(category.to_owned()).or_default();
    total.count += count;
    if let Some(example) = example {
        total.examples.insert(example.to_owned());
    }
}

fn example_path(result: &FileMetrics) -> Option<&str> {
    (!result.private).then_some(result.path.as_str())
}

fn render_section(
    output: &mut String,
    title: &str,
    categories: BTreeMap<String, CategoryAggregate>,
) {
    writeln!(output, "\n## {title}\n").unwrap();
    if categories.is_empty() {
        writeln!(output, "No disagreements observed.").unwrap();
        return;
    }

    writeln!(output, "| Category | Occurrences | Example files |").unwrap();
    writeln!(output, "| --- | ---: | --- |").unwrap();
    let mut categories: Vec<_> = categories.into_iter().collect();
    categories.sort_by(|left, right| {
        right
            .1
            .count
            .cmp(&left.1.count)
            .then_with(|| left.0.cmp(&right.0))
    });
    for (category, aggregate) in categories.into_iter().take(MAX_CATEGORIES) {
        let examples = aggregate
            .examples
            .iter()
            .take(MAX_EXAMPLES)
            .map(|example| inline_code(example))
            .collect::<Vec<_>>()
            .join(", ");
        let examples = if examples.is_empty() {
            "—".to_owned()
        } else {
            examples
        };
        writeln!(
            output,
            "| {} | {} | {} |",
            inline_code(&category),
            aggregate.count,
            examples
        )
        .unwrap();
    }
}

fn inline_code(value: &str) -> String {
    let escaped = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace(['\r', '\n'], " ");
    format!("<code>{escaped}</code>")
}

#[cfg(test)]
mod tests {
    use super::render_triage;
    use crate::compare::{CountMetric, CoverageMetric, FileMetrics, MismatchBucket, ToolSummary};
    use crate::model::DumpError;
    use crate::roundtrip::{RoundTripFailure, RoundTripFileMetrics};

    fn file(path: &str, private: bool) -> FileMetrics {
        FileMetrics {
            id: path.to_owned(),
            path: path.to_owned(),
            sha256: "abc".to_owned(),
            ext: "xlsx".to_owned(),
            private,
            wax: ToolSummary {
                ok: false,
                error: Some(DumpError {
                    code: "bad_zip".to_owned(),
                    msg: "fixture".to_owned(),
                }),
                wall_ms: None,
                peak_rss_bytes: None,
                truncated: false,
            },
            sheetjs: ToolSummary {
                ok: true,
                error: None,
                wall_ms: None,
                peak_rss_bytes: None,
                truncated: false,
            },
            serve: None,
            round_trip: None,
            cell_value_match: CountMetric::default(),
            wax_display_coverage: CoverageMetric::default(),
            sheetjs_display_coverage: CoverageMetric::default(),
            display_string_match: CountMetric::default(),
            formula_fidelity: CountMetric::default(),
            cached_result_fidelity: CountMetric::default(),
            format_display: Vec::new(),
            value_mismatches: vec![MismatchBucket {
                category: "wax:n / SheetJS:s".to_owned(),
                count: 2,
            }],
            display_mismatches: vec![MismatchBucket {
                category: "#,##0.00|kr".to_owned(),
                count: 1,
            }],
            warnings: Vec::new(),
        }
    }

    #[test]
    fn triage_counts_private_results_without_leaking_their_paths() {
        let results = vec![
            file("public/example.xlsx", false),
            file("/private/secret-ledger.xlsx", true),
        ];

        let markdown = render_triage(&results, "2026-07-28T00:00:00Z");

        assert!(markdown.contains("<code>bad_zip</code> | 2"));
        assert!(markdown.contains("<code>public/example.xlsx</code>"));
        assert!(!markdown.contains("secret-ledger"));
        assert!(markdown.contains("<code>wax:n / SheetJS:s</code> | 4"));
        assert!(markdown.contains("<code>#,##0.00&#124;kr</code> | 2"));
    }

    #[test]
    fn empty_triage_reports_each_category_as_clear() {
        let markdown = render_triage(&[], "2026-07-28T00:00:00Z");

        assert_eq!(markdown.matches("No disagreements observed.").count(), 7);
    }

    #[test]
    fn oracle_triage_counts_private_failures_without_leaking_paths() {
        let mut result = file("/private/secret-ledger.xlsx", true);
        result.round_trip = Some(RoundTripFileMetrics {
            status: "clean".to_owned(),
            oracle_open: Some(false),
            oracle_error: Some(RoundTripFailure {
                stage: "oracleReadBack".to_owned(),
                code: "bad_zip".to_owned(),
                msg: "private details".to_owned(),
            }),
            ..RoundTripFileMetrics::default()
        });

        let markdown = render_triage(&[result], "2026-07-28T00:00:00Z");

        assert!(markdown
            .contains("1 private file(s) had oracle read-back failures; paths are omitted."));
        assert!(!markdown.contains("secret-ledger"));
        assert!(!markdown.contains("private details"));
    }

    #[test]
    fn round_trip_triage_surfaces_export_drops_and_internal_failures() {
        let mut dropped = file("public/truncated.xls", false);
        dropped.round_trip = Some(RoundTripFileMetrics {
            status: "defect".to_owned(),
            export_dropped: vec![
                "cell A1 string truncated from 32768 to 32767 characters".to_owned()
            ],
            ..RoundTripFileMetrics::default()
        });
        let mut failed = file("public/regression.xls", false);
        failed.round_trip = Some(RoundTripFileMetrics {
            status: "failed".to_owned(),
            error: Some(RoundTripFailure {
                stage: "export".to_owned(),
                code: "internal".to_owned(),
                msg: "live writer regression".to_owned(),
            }),
            ..RoundTripFileMetrics::default()
        });

        let markdown = render_triage(&[dropped, failed], "2026-07-28T00:00:00Z");

        assert!(markdown.contains("## Round-trip export drops"));
        assert!(markdown.contains("cell A1 string truncated from 32768 to 32767 characters"));
        assert!(markdown.contains("## Round-trip failures"));
        assert!(markdown.contains("export: internal: live writer regression"));
    }
}

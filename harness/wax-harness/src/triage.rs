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
    output
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

        assert_eq!(markdown.matches("No disagreements observed.").count(), 3);
    }
}

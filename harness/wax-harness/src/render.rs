use std::fmt::Write;

use crate::aggregate::{RatioMetric, Scoreboard};
use crate::formats::FormatCoverageReport;

pub fn render_markdown(scoreboard: &Scoreboard) -> String {
    render_markdown_with_formats(scoreboard, None)
}

pub fn render_markdown_with_formats(
    scoreboard: &Scoreboard,
    format_coverage: Option<&FormatCoverageReport>,
) -> String {
    let metrics = &scoreboard.metrics;
    let mut output = String::new();
    writeln!(output, "# wax compatibility scoreboard\n").unwrap();
    writeln!(output, "Generated: `{}`\n", scoreboard.generated_at).unwrap();
    writeln!(
        output,
        "Corpus: {} attempted, {} skipped.\n",
        scoreboard.files_attempted, scoreboard.files_skipped
    )
    .unwrap();
    writeln!(output, "| Metric | wax | SheetJS baseline |").unwrap();
    writeln!(output, "| --- | ---: | ---: |").unwrap();
    writeln!(
        output,
        "| files opened % | {} | {} |",
        ratio(&metrics.files_opened.wax),
        ratio(&metrics.files_opened.sheetjs)
    )
    .unwrap();
    writeln!(
        output,
        "| cell-value match % | {} | {} |",
        ratio(&metrics.cell_value_match),
        baseline(&metrics.cell_value_match)
    )
    .unwrap();
    writeln!(
        output,
        "| display-string coverage % | {} | {} |",
        ratio(&metrics.display_string_coverage.wax),
        ratio(&metrics.display_string_coverage.sheetjs)
    )
    .unwrap();
    writeln!(
        output,
        "| display-string match % | {} | {} |",
        ratio(&metrics.display_string_match),
        baseline(&metrics.display_string_match)
    )
    .unwrap();
    writeln!(
        output,
        "| formula fidelity % | {} | {} |",
        ratio(&metrics.formula_fidelity),
        baseline(&metrics.formula_fidelity)
    )
    .unwrap();
    writeln!(
        output,
        "| cached-result fidelity % | {} | {} |",
        ratio(&metrics.cached_result_fidelity),
        baseline(&metrics.cached_result_fidelity)
    )
    .unwrap();
    writeln!(
        output,
        "| p50 parse time | {} | {} |",
        milliseconds(metrics.parse_time_ms.wax.p50),
        milliseconds(metrics.parse_time_ms.sheetjs.p50)
    )
    .unwrap();
    writeln!(
        output,
        "| p95 parse time | {} | {} |",
        milliseconds(metrics.parse_time_ms.wax.p95),
        milliseconds(metrics.parse_time_ms.sheetjs.p95)
    )
    .unwrap();
    writeln!(
        output,
        "| peak RSS (p50 / max) | {} | {} |",
        rss(
            metrics.peak_rss_bytes.wax.p50,
            metrics.peak_rss_bytes.wax.max
        ),
        rss(
            metrics.peak_rss_bytes.sheetjs.p50,
            metrics.peak_rss_bytes.sheetjs.max
        )
    )
    .unwrap();
    writeln!(output, "| window latency | n/a | n/a |").unwrap();
    render_extensions(&mut output, scoreboard);
    if let Some(format_coverage) = format_coverage {
        render_formats(&mut output, format_coverage);
    }
    output
}

fn render_extensions(output: &mut String, scoreboard: &Scoreboard) {
    writeln!(output, "\n## Per-extension compatibility\n").unwrap();
    writeln!(output, "The `xlsx` row is the binding W2 reader gate.\n").unwrap();
    if scoreboard.metrics.per_extension.is_empty() {
        writeln!(output, "No extension data was observed.").unwrap();
        return;
    }

    writeln!(
        output,
        "| Extension | Files attempted | wax opened | SheetJS opened | Cell-value match |"
    )
    .unwrap();
    writeln!(output, "| --- | ---: | ---: | ---: | ---: |").unwrap();
    for (extension, metrics) in &scoreboard.metrics.per_extension {
        let label = if extension == "xlsx" {
            "<code>xlsx</code> (W2 gate)".to_owned()
        } else {
            inline_code(extension)
        };
        writeln!(
            output,
            "| {} | {} | {} | {} | {} |",
            label,
            metrics.files_attempted,
            ratio(&metrics.files_opened.wax),
            ratio(&metrics.files_opened.sheetjs),
            ratio(&metrics.cell_value_match)
        )
        .unwrap();
    }
}

fn render_formats(output: &mut String, report: &FormatCoverageReport) {
    writeln!(output, "\n## Top format-code display compatibility\n").unwrap();
    if report.formats.is_empty() {
        writeln!(output, "No non-General oracle format codes were observed.").unwrap();
        return;
    }

    let ranking = if report.joined_corpus_formats {
        "corpus-wide cell count from `harness/formats/corpus-formats.json`"
    } else {
        "cell count observed in this run (corpus format ranking was unavailable)"
    };
    writeln!(output, "Top 20 ranked by {ranking}.\n").unwrap();
    writeln!(
        output,
        "| Format code | Oracle cells (run / corpus) | wax display coverage | Display match |"
    )
    .unwrap();
    writeln!(output, "| --- | ---: | ---: | ---: |").unwrap();
    for format in report.formats.iter().take(20) {
        let corpus_cells = format
            .corpus_cell_count
            .map(|count| count.to_string())
            .unwrap_or_else(|| "n/a".to_owned());
        writeln!(
            output,
            "| {} | {} / {} | {} | {} |",
            inline_code(&format.code),
            format.cell_count,
            corpus_cells,
            ratio(&format.wax_display_coverage),
            ratio(&format.display_string_match)
        )
        .unwrap();
    }
}

fn ratio(metric: &RatioMetric) -> String {
    metric
        .percent
        .map(|percent| format!("{percent:.2}% ({}/{})", metric.matched, metric.total))
        .unwrap_or_else(|| "n/a".to_owned())
}

fn baseline(metric: &RatioMetric) -> &'static str {
    if metric.total == 0 {
        "n/a"
    } else {
        "reference"
    }
}

fn milliseconds(value: Option<u64>) -> String {
    value
        .map(|value| format!("{value} ms"))
        .unwrap_or_else(|| "n/a".to_owned())
}

fn rss(p50: Option<u64>, max: Option<u64>) -> String {
    match (p50, max) {
        (Some(p50), Some(max)) => format!("{} / {}", bytes(p50), bytes(max)),
        _ => "n/a".to_owned(),
    }
}

fn bytes(value: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    if value >= GIB {
        format!("{:.2} GiB", value as f64 / GIB as f64)
    } else if value >= MIB {
        format!("{:.2} MiB", value as f64 / MIB as f64)
    } else if value >= KIB {
        format!("{:.2} KiB", value as f64 / KIB as f64)
    } else {
        format!("{value} B")
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

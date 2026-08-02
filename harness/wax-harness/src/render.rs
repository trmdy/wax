use std::fmt::Write;

use crate::aggregate::{RatioMetric, RoundTripMetrics, Scoreboard};
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
        "| open-via-serve % | {} | n/a |",
        serve_ratio(scoreboard, metrics.open_via_serve.as_ref())
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
        "| formula cells evaluated % (covered set) | {} | n/a |",
        serve_ratio(scoreboard, Some(&metrics.formula_cells_evaluated))
    )
    .unwrap();
    writeln!(
        output,
        "| evaluated-vs-file-cached agreement % | {} | n/a |",
        serve_ratio(scoreboard, Some(&metrics.evaluated_cache_agreement))
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
    writeln!(
        output,
        "| serve peak RSS (p50 / max) | {} | n/a |",
        serve_rss(scoreboard)
    )
    .unwrap();
    writeln!(
        output,
        "| window latency (p50 / p95) | {} | n/a |",
        serve_latency(scoreboard)
    )
    .unwrap();
    if let Some(round_trip) = metrics.round_trip.as_ref() {
        render_round_trip(&mut output, round_trip);
    }
    render_extensions(&mut output, scoreboard);
    if let Some(format_coverage) = format_coverage {
        render_formats(&mut output, format_coverage);
    }
    output
}

fn render_round_trip(output: &mut String, metrics: &RoundTripMetrics) {
    writeln!(output, "\n## Writer round-trip\n").unwrap();
    writeln!(output, "| Metric | Result |").unwrap();
    writeln!(output, "| --- | ---: |").unwrap();
    writeln!(
        output,
        "| round-trip files clean % | {} |",
        round_trip_ratio(metrics, &metrics.files_clean)
    )
    .unwrap();
    writeln!(
        output,
        "| round-trip value fidelity % | {} |",
        round_trip_ratio(metrics, &metrics.value_match)
    )
    .unwrap();
    writeln!(
        output,
        "| round-trip display fidelity % | {} |",
        round_trip_ratio(metrics, &metrics.display_match)
    )
    .unwrap();
    writeln!(
        output,
        "| oracle read-back open % | {} |",
        round_trip_ratio(metrics, &metrics.oracle_open_rate)
    )
    .unwrap();
    writeln!(output, "| soffice-open rate | {} |", soffice_ratio(metrics)).unwrap();
    writeln!(
        output,
        "| truncated models skipped | {} |",
        metrics.skipped_truncated
    )
    .unwrap();
}

fn round_trip_ratio(metrics: &RoundTripMetrics, metric: &RatioMetric) -> String {
    if metrics.status.status == "unavailable" {
        "n/a (xlsx export unavailable)".to_owned()
    } else {
        ratio(metric)
    }
}

fn soffice_ratio(metrics: &RoundTripMetrics) -> String {
    if metrics.status.status == "unavailable" {
        return "n/a (xlsx export unavailable)".to_owned();
    }
    match metrics.status.soffice_status.as_str() {
        "disabled" => "n/a (soffice disabled)".to_owned(),
        "unavailable" => "n/a (soffice unavailable)".to_owned(),
        _ => ratio(&metrics.soffice_open_rate),
    }
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
        "| Extension | Files attempted | wax opened | SheetJS opened | Cell-value match | Formula-text fidelity | Cached-result fidelity |"
    )
    .unwrap();
    writeln!(output, "| --- | ---: | ---: | ---: | ---: | ---: | ---: |").unwrap();
    for (extension, metrics) in &scoreboard.metrics.per_extension {
        let label = if extension == "xlsx" {
            "<code>xlsx</code> (W2 gate)".to_owned()
        } else {
            inline_code(extension)
        };
        writeln!(
            output,
            "| {} | {} | {} | {} | {} | {} | {} |",
            label,
            metrics.files_attempted,
            ratio(&metrics.files_opened.wax),
            ratio(&metrics.files_opened.sheetjs),
            ratio(&metrics.cell_value_match),
            ratio(&metrics.formula_fidelity),
            ratio(&metrics.cached_result_fidelity)
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

fn serve_ratio(scoreboard: &Scoreboard, metric: Option<&RatioMetric>) -> String {
    match scoreboard.metrics.serve_status.status.as_str() {
        "unavailable" => "n/a (serve unavailable)".to_owned(),
        "disabled" => "n/a (serve disabled)".to_owned(),
        _ => metric.map(ratio).unwrap_or_else(|| "n/a".to_owned()),
    }
}

fn serve_latency(scoreboard: &Scoreboard) -> String {
    match scoreboard.metrics.serve_status.status.as_str() {
        "unavailable" => "n/a (serve unavailable)".to_owned(),
        "disabled" => "n/a (serve disabled)".to_owned(),
        _ => match (
            scoreboard.metrics.window_latency_percentiles_ms.p50,
            scoreboard.metrics.window_latency_percentiles_ms.p95,
        ) {
            (Some(p50), Some(p95)) => {
                format!("{p50:.3} ms / {p95:.3} ms")
            }
            _ => "n/a".to_owned(),
        },
    }
}

fn serve_rss(scoreboard: &Scoreboard) -> String {
    match scoreboard.metrics.serve_status.status.as_str() {
        "unavailable" => "n/a (serve unavailable)".to_owned(),
        "disabled" => "n/a (serve disabled)".to_owned(),
        _ => rss(
            scoreboard.metrics.serve_peak_rss_bytes.p50,
            scoreboard.metrics.serve_peak_rss_bytes.max,
        ),
    }
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

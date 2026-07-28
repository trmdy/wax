use std::fmt::Write;

use crate::aggregate::{RatioMetric, Scoreboard};

pub fn render_markdown(scoreboard: &Scoreboard) -> String {
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
    output
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

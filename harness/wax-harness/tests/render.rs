use wax_harness::{render_markdown, Scoreboard};

#[test]
fn scoreboard_markdown_matches_golden_file() {
    let scoreboard: Scoreboard = serde_json::from_value(serde_json::json!({
        "schema": 1,
        "generatedAt": "2026-07-28T00:00:00Z",
        "filesAttempted": 3,
        "filesSkipped": 1,
        "metrics": {
            "filesOpened": {
                "wax": {"matched": 2, "total": 3, "percent": 66.6666666667},
                "sheetjs": {"matched": 3, "total": 3, "percent": 100.0}
            },
            "cellValueMatch": {"matched": 1, "total": 2, "percent": 50.0},
            "displayStringCoverage": {
                "wax": {"matched": 0, "total": 2, "percent": 0.0},
                "sheetjs": {"matched": 2, "total": 2, "percent": 100.0}
            },
            "formulaFidelity": {"matched": 1, "total": 1, "percent": 100.0},
            "cachedResultFidelity": {"matched": 0, "total": 1, "percent": 0.0},
            "parseTimeMs": {
                "wax": {"p50": 10, "p95": 12},
                "sheetjs": {"p50": 20, "p95": 22}
            },
            "peakRssBytes": {
                "wax": {"p50": 100, "max": 120},
                "sheetjs": {"p50": 200, "max": 220}
            },
            "windowLatencyMs": {"wax": null, "sheetjs": null}
        }
    }))
    .unwrap();

    assert_eq!(
        render_markdown(&scoreboard),
        include_str!("fixtures/scoreboard.golden.md")
    );
}

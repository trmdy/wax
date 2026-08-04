use std::fs;
use std::path::Path;

use wax_harness::{render_markdown_with_formats, FormatCoverageReport, Scoreboard};

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
            "displayStringMatch": {"matched": 1, "total": 2, "percent": 50.0},
            "formulaFidelity": {"matched": 1, "total": 1, "percent": 100.0},
            "cachedResultFidelity": {"matched": 0, "total": 1, "percent": 0.0},
            "formulaCellsEvaluated": {"matched": 9, "total": 10, "percent": 90.0},
            "evaluatedCacheAgreement": {"matched": 8, "total": 9, "percent": 88.8888888889},
            "parseTimeMs": {
                "wax": {"p50": 10, "p95": 12},
                "sheetjs": {"p50": 20, "p95": 22}
            },
            "peakRssBytes": {
                "wax": {"p50": 100, "max": 120},
                "sheetjs": {"p50": 200, "max": 220}
            },
            "windowLatencyMs": {"wax": null, "sheetjs": null},
            "openViaServe": {"matched": 2, "total": 3, "percent": 66.6666666667},
            "windowLatencyPercentilesMs": {"p50": 0.25, "p95": 1.75},
            "servePeakRssBytes": {"p50": 110, "max": 130},
            "serveStatus": {"status": "available", "reason": null},
            "perExtension": {
                "ods": {
                    "filesAttempted": 1,
                    "filesOpened": {
                        "wax": {"matched": 0, "total": 1, "percent": 0.0},
                        "sheetjs": {"matched": 1, "total": 1, "percent": 100.0}
                    },
                    "cellValueMatch": {"matched": 0, "total": 0, "percent": null},
                    "formulaFidelity": {"matched": 0, "total": 0, "percent": null},
                    "cachedResultFidelity": {"matched": 0, "total": 0, "percent": null}
                },
                "xlsx": {
                    "filesAttempted": 2,
                    "filesOpened": {
                        "wax": {"matched": 2, "total": 2, "percent": 100.0},
                        "sheetjs": {"matched": 2, "total": 2, "percent": 100.0}
                    },
                    "cellValueMatch": {"matched": 1, "total": 2, "percent": 50.0},
                    "formulaFidelity": {"matched": 1, "total": 1, "percent": 100.0},
                    "cachedResultFidelity": {"matched": 0, "total": 1, "percent": 0.0}
                }
            }
        }
    }))
    .unwrap();
    let formats: FormatCoverageReport = serde_json::from_value(serde_json::json!({
        "schema": 1,
        "generatedAt": "2026-07-28T00:00:00Z",
        "filesAttempted": 3,
        "ranking": "corpusCellCount",
        "joinedCorpusFormats": true,
        "totals": {
            "formatCodes": 1,
            "oracleCells": 2,
            "waxDisplayCoverage": {"matched": 1, "total": 2, "percent": 50.0},
            "displayStringMatch": {"matched": 1, "total": 1, "percent": 100.0}
        },
        "formats": [{
            "code": "#,##0.00|kr",
            "cellCount": 2,
            "fileCount": 1,
            "corpusCellCount": 50,
            "corpusFileCount": 10,
            "waxDisplayCoverage": {"matched": 1, "total": 2, "percent": 50.0},
            "displayStringMatch": {"matched": 1, "total": 1, "percent": 100.0}
        }]
    }))
    .unwrap();

    assert_eq!(
        render_markdown_with_formats(&scoreboard, Some(&formats)),
        include_str!("fixtures/scoreboard.golden.md")
    );
}

#[test]
fn committed_scoreboard_snapshot_matches_its_json_sources() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scoreboard: Scoreboard =
        serde_json::from_slice(&fs::read(repo_root.join("harness/scoreboard.json")).unwrap())
            .unwrap();
    let formats: FormatCoverageReport =
        serde_json::from_slice(&fs::read(repo_root.join("harness/format-coverage.json")).unwrap())
            .unwrap();

    assert_eq!(
        render_markdown_with_formats(&scoreboard, Some(&formats)),
        fs::read_to_string(repo_root.join("SCOREBOARD.md")).unwrap()
    );
}

#[test]
fn committed_scoreboard_json_round_trips_through_the_typed_model_losslessly() {
    // Originally also asserted `round_trip.is_none()`, which was only true
    // before the W4 writer landed real round-trip numbers in the committed
    // snapshot. The durable property is that the typed model loses nothing:
    // absent-roundTrip legacy stability is pinned by the runner integration
    // fixtures instead.
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let original = fs::read(repo_root.join("harness/scoreboard.json")).unwrap();
    let scoreboard: Scoreboard = serde_json::from_slice(&original).unwrap();

    let original: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let rendered = serde_json::to_value(&scoreboard).unwrap();
    assert_eq!(rendered, original);
}

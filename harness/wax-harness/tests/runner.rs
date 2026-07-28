use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn prepare_repo() -> TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("files")).unwrap();
    fs::create_dir_all(root.path().join("harness/oracle")).unwrap();
    for name in ["crash.xlsx", "diff.xlsx", "match.xlsx"] {
        fs::copy(
            fixture(&format!("files/{name}")),
            root.path().join("files").join(name),
        )
        .unwrap();
    }
    fs::copy(
        fixture("manifest.jsonl"),
        root.path().join("manifest.jsonl"),
    )
    .unwrap();
    root
}

fn harness_command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wax-harness"));
    command
        .arg("--repo-root")
        .arg(root)
        .arg("--manifest")
        .arg("manifest.jsonl")
        .arg("--jobs")
        .arg("2")
        .env("WAX_BIN", fixture("fake-wax.sh"))
        .env("WAX_ORACLE_SCRIPT", fixture("fake-oracle.js"));
    command
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "status={}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn runner_records_three_fixture_files_including_a_tool_crash() {
    let root = prepare_repo();
    let output = harness_command(root.path()).output().unwrap();
    assert_success(output);

    let result_lines = fs::read_to_string(root.path().join("harness/results.jsonl")).unwrap();
    let results: Vec<Value> = result_lines
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(results.len(), 3);
    let crash = results
        .iter()
        .find(|result| result["id"] == "fixtures/crash.xlsx")
        .unwrap();
    assert_eq!(crash["wax"]["ok"], false);
    assert_eq!(crash["wax"]["error"]["code"], "process_exit");
    assert_eq!(crash["sheetjs"]["ok"], true);

    let scoreboard: Value =
        serde_json::from_slice(&fs::read(root.path().join("harness/scoreboard.json")).unwrap())
            .unwrap();
    assert_eq!(scoreboard["filesAttempted"], 3);
    assert_eq!(scoreboard["metrics"]["filesOpened"]["wax"]["matched"], 2);
    assert_eq!(scoreboard["metrics"]["cellValueMatch"]["matched"], 1);
    assert_eq!(scoreboard["metrics"]["cellValueMatch"]["total"], 2);
    assert_eq!(scoreboard["metrics"]["displayStringMatch"]["matched"], 1);
    assert_eq!(scoreboard["metrics"]["displayStringMatch"]["total"], 2);
    assert_eq!(
        scoreboard["metrics"]["perExtension"]["xlsx"]["filesAttempted"],
        2
    );
    assert_eq!(
        scoreboard["metrics"]["perExtension"]["ods"]["filesAttempted"],
        1
    );
    assert_eq!(scoreboard["metrics"]["windowLatencyMs"]["wax"], Value::Null);
    assert_eq!(scoreboard["metrics"]["openViaServe"], Value::Null);
    assert_eq!(
        scoreboard["metrics"]["serveStatus"]["status"],
        "unavailable"
    );
    assert_eq!(
        scoreboard["metrics"]["roundTrip"]["status"]["status"],
        "unavailable"
    );
    assert_eq!(
        scoreboard["metrics"]["roundTrip"]["status"]["reason"],
        "xlsx export unavailable"
    );
    assert_eq!(
        scoreboard["metrics"]["roundTrip"]["filesClean"]["percent"],
        Value::Null
    );
    assert_eq!(
        scoreboard["metrics"]["perExtension"]["ods"]["formulaFidelity"]["matched"],
        1
    );
    assert_eq!(
        scoreboard["metrics"]["perExtension"]["xlsx"]["formulaFidelity"]["total"],
        0
    );
    let formats: Value = serde_json::from_slice(
        &fs::read(root.path().join("harness/format-coverage.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(formats["joinedCorpusFormats"], false);
    assert_eq!(formats["formats"][0]["code"], "0.00");
    assert_eq!(formats["formats"][0]["cellCount"], 2);
    assert_eq!(formats["formats"][0]["waxDisplayCoverage"]["matched"], 2);
    assert_eq!(formats["formats"][0]["displayStringMatch"]["matched"], 1);
    assert_eq!(formats["formats"][0]["displayStringMatch"]["total"], 2);
    let triage = fs::read_to_string(root.path().join("harness/triage.md")).unwrap();
    assert!(triage.contains("<code>process_exit</code>"));
    assert!(triage.contains("<code>wax:n / SheetJS:n</code>"));
    assert!(triage.contains("<code>0.00</code>"));
    assert!(triage.contains("<code>files/diff.xlsx</code>"));
    assert!(root.path().join("SCOREBOARD.md").is_file());
}

#[test]
fn run_sh_is_an_end_to_end_entry_point_for_the_fake_contract_tools() {
    let root = prepare_repo();
    let run_sh = Path::new(env!("CARGO_MANIFEST_DIR")).join("../run.sh");
    let output = Command::new(run_sh)
        .arg("--manifest")
        .arg("manifest.jsonl")
        .arg("--limit")
        .arg("3")
        .arg("--jobs")
        .arg("2")
        .env("WAX_REPO_ROOT", root.path())
        .env("WAX_BIN", fixture("fake-wax.sh"))
        .env("WAX_ORACLE_SCRIPT", fixture("fake-oracle.js"))
        .env("WAX_HARNESS_BIN", env!("CARGO_BIN_EXE_wax-harness"))
        .output()
        .unwrap();
    assert_success(output);

    assert_eq!(
        fs::read_to_string(root.path().join("harness/results.jsonl"))
            .unwrap()
            .lines()
            .count(),
        3
    );
    let markdown = fs::read_to_string(root.path().join("SCOREBOARD.md")).unwrap();
    assert!(markdown.contains("| window latency (p50 / p95) | n/a (serve unavailable) | n/a |"));
    assert!(markdown.contains("| serve peak RSS (p50 / max) | n/a (serve unavailable) | n/a |"));
    assert!(markdown.contains("<code>xlsx</code> (W2 gate)"));
    assert!(markdown.contains("## Top format-code display compatibility"));
    assert!(markdown.contains("## Writer round-trip"));
    assert!(markdown.contains("n/a (xlsx export unavailable)"));
    assert!(root.path().join("harness/format-coverage.json").is_file());
    assert!(root.path().join("harness/triage.md").is_file());
}

#[test]
fn runner_collects_round_trip_oracle_and_soffice_metrics_with_mocked_export() {
    let root = prepare_repo();
    let output = harness_command(root.path())
        .arg("--soffice")
        .env("WAX_BIN", fixture("fake-wax-export.sh"))
        .env("WAX_SOFFICE_BIN", fixture("fake-soffice.sh"))
        .output()
        .unwrap();
    assert_success(output);

    let scoreboard: Value =
        serde_json::from_slice(&fs::read(root.path().join("harness/scoreboard.json")).unwrap())
            .unwrap();
    let round_trip = &scoreboard["metrics"]["roundTrip"];
    assert_eq!(round_trip["status"]["status"], "available");
    assert_eq!(round_trip["status"]["sofficeStatus"], "available");
    assert_eq!(round_trip["filesClean"]["matched"], 1);
    assert_eq!(round_trip["filesClean"]["total"], 2);
    assert_eq!(round_trip["valueMatch"]["matched"], 1);
    assert_eq!(round_trip["valueMatch"]["total"], 1);
    assert_eq!(round_trip["displayMatch"]["matched"], 1);
    assert_eq!(round_trip["displayMatch"]["total"], 1);
    assert_eq!(round_trip["oracleOpenRate"]["matched"], 1);
    assert_eq!(round_trip["oracleOpenRate"]["total"], 1);
    assert_eq!(round_trip["sofficeOpenRate"]["matched"], 1);
    assert_eq!(round_trip["sofficeOpenRate"]["total"], 1);
    assert_eq!(round_trip["skippedTruncated"], 0);

    let result_lines = fs::read_to_string(root.path().join("harness/results.jsonl")).unwrap();
    let results: Vec<Value> = result_lines
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let clean = results
        .iter()
        .find(|result| result["id"] == "fixtures/match.xlsx")
        .unwrap();
    assert_eq!(clean["roundTrip"]["status"], "clean");
    let failed = results
        .iter()
        .find(|result| result["id"] == "fixtures/diff.xlsx")
        .unwrap();
    assert_eq!(failed["roundTrip"]["status"], "failed");
    assert_eq!(failed["roundTrip"]["error"]["stage"], "export");

    let markdown = fs::read_to_string(root.path().join("SCOREBOARD.md")).unwrap();
    assert!(markdown.contains("| round-trip files clean % | 50.00% (1/2) |"));
    assert!(markdown.contains("| oracle read-back open % | 100.00% (1/1) |"));
    assert!(markdown.contains("| soffice-open rate | 100.00% (1/1) |"));
}

#[test]
fn structured_internal_export_error_degrades_the_whole_section_to_na() {
    let root = prepare_repo();
    let output = harness_command(root.path())
        .env("WAX_BIN", fixture("fake-wax-export.sh"))
        .env("WAX_FAKE_EXPORT_INTERNAL", "1")
        .output()
        .unwrap();
    assert_success(output);

    let scoreboard: Value =
        serde_json::from_slice(&fs::read(root.path().join("harness/scoreboard.json")).unwrap())
            .unwrap();
    assert_eq!(
        scoreboard["metrics"]["roundTrip"]["status"]["status"],
        "unavailable"
    );
    assert_eq!(scoreboard["metrics"]["roundTrip"]["filesClean"]["total"], 2);
    assert_eq!(
        scoreboard["metrics"]["roundTrip"]["filesClean"]["percent"],
        Value::Null
    );
    let markdown = fs::read_to_string(root.path().join("SCOREBOARD.md")).unwrap();
    assert_eq!(markdown.matches("n/a (xlsx export unavailable)").count(), 5);
}

#[test]
fn enabled_but_missing_soffice_is_reported_as_na() {
    let root = prepare_repo();
    let output = harness_command(root.path())
        .arg("--soffice")
        .env("WAX_BIN", fixture("fake-wax-export.sh"))
        .env("WAX_SOFFICE_BIN", root.path().join("missing-soffice"))
        .output()
        .unwrap();
    assert_success(output);

    let scoreboard: Value =
        serde_json::from_slice(&fs::read(root.path().join("harness/scoreboard.json")).unwrap())
            .unwrap();
    assert_eq!(
        scoreboard["metrics"]["roundTrip"]["status"]["sofficeStatus"],
        "unavailable"
    );
    assert_eq!(
        scoreboard["metrics"]["roundTrip"]["sofficeOpenRate"]["percent"],
        Value::Null
    );
    let markdown = fs::read_to_string(root.path().join("SCOREBOARD.md")).unwrap();
    assert!(markdown.contains("| soffice-open rate | n/a (soffice unavailable) |"));
}

#[test]
fn runner_joins_the_optional_corpus_format_ranking() {
    let root = prepare_repo();
    fs::create_dir_all(root.path().join("harness/formats")).unwrap();
    fs::write(
        root.path().join("harness/formats/corpus-formats.json"),
        r#"{
          "formats": [
            {"code": "0.00", "cellCount": 1234, "fileCount": 99}
          ]
        }"#,
    )
    .unwrap();

    let output = harness_command(root.path()).output().unwrap();
    assert_success(output);

    let formats: Value = serde_json::from_slice(
        &fs::read(root.path().join("harness/format-coverage.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(formats["joinedCorpusFormats"], true);
    assert_eq!(formats["ranking"], "corpusCellCount");
    assert_eq!(formats["formats"][0]["corpusCellCount"], 1234);
    assert_eq!(formats["formats"][0]["corpusFileCount"], 99);
}

#[test]
fn malformed_tool_json_becomes_a_schema_violation_row() {
    let root = prepare_repo();
    let bad_wax = root.path().join("bad-wax.sh");
    fs::write(&bad_wax, "#!/bin/sh\nprintf 'not-json'\n").unwrap();
    make_executable(&bad_wax);

    let output = harness_command(root.path())
        .env("WAX_BIN", &bad_wax)
        .output()
        .unwrap();
    assert_success(output);

    let results = fs::read_to_string(root.path().join("harness/results.jsonl")).unwrap();
    for line in results.lines() {
        let result: Value = serde_json::from_str(line).unwrap();
        assert_eq!(result["wax"]["ok"], false);
        assert_eq!(result["wax"]["error"]["code"], "schema_violation");
    }
}

#[test]
fn timed_out_tool_becomes_a_timeout_row() {
    let root = prepare_repo();
    let slow_wax = root.path().join("slow-wax.sh");
    fs::write(&slow_wax, "#!/bin/sh\nexec sleep 5\n").unwrap();
    make_executable(&slow_wax);

    let output = harness_command(root.path())
        .arg("--limit")
        .arg("1")
        .arg("--timeout-ms")
        .arg("50")
        .env("WAX_BIN", &slow_wax)
        .output()
        .unwrap();
    assert_success(output);

    let results = fs::read_to_string(root.path().join("harness/results.jsonl")).unwrap();
    let result: Value = serde_json::from_str(results.trim()).unwrap();
    assert_eq!(result["wax"]["ok"], false);
    assert_eq!(result["wax"]["error"]["code"], "timeout");
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

use std::path::{Path, PathBuf};
use std::process::Command;

fn wax() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wax"))
}

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("wax-read")
        .join("tests")
        .join("fixtures")
        .join("reader.xlsx")
}

#[test]
fn version_includes_semver_and_protocol() {
    let output = wax().arg("--version").output().expect("wax should execute");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "wax 0.1.0 (proto 0)\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn dump_emits_normalized_json_and_metrics() {
    let path = fixture_path();
    let output = wax()
        .args(["dump", "--json"])
        .arg(&path)
        .args(["--max-cells", "2", "--timeout-ms", "10000"])
        .output()
        .expect("wax should execute");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let dump: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should contain JSON");
    assert_eq!(dump["schema"], 1);
    assert_eq!(dump["tool"], "wax");
    assert_eq!(dump["toolVersion"], "0.1.0");
    assert_eq!(dump["ok"], true);
    assert_eq!(dump["truncated"], true);
    assert_eq!(dump["sheets"][0]["cells"].as_array().unwrap().len(), 2);
    assert_eq!(dump["sha256"].as_str().unwrap().len(), 64);
    assert!(dump["wallMs"].is_u64());
    assert!(dump.get("peakRssBytes").is_some());
}

#[test]
fn unsupported_file_still_exits_zero_with_failure_document() {
    let temp = tempfile::tempdir().expect("temporary directory should be created");
    let path = temp.path().join("values.csv");
    std::fs::write(&path, "one,two\n").expect("fixture should be written");
    let output = wax()
        .args(["dump", "--json"])
        .arg(path)
        .output()
        .expect("wax should execute");

    assert!(output.status.success());
    let dump: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should contain JSON");
    assert_eq!(dump["ok"], false);
    assert_eq!(dump["error"]["code"], "unsupported");
}

#[test]
fn max_bytes_is_a_structured_failure() {
    let output = wax()
        .args(["dump", "--json"])
        .arg(fixture_path())
        .args(["--max-bytes", "1"])
        .output()
        .expect("wax should execute");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let dump: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should contain JSON");
    assert_eq!(dump["ok"], false);
    assert_eq!(dump["error"]["code"], "too_large");
}

#[test]
fn usage_error_exits_two_without_stdout() {
    let output = wax()
        .args(["dump", "missing.xlsx"])
        .output()
        .expect("wax should execute");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires --json"));
}

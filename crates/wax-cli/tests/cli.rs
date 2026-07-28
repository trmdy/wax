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

fn json_stdout(output: &std::process::Output) -> serde_json::Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.lines().count(),
        1,
        "expected exactly one JSON line: {stdout:?}"
    );
    serde_json::from_slice(&output.stdout).expect("stdout should contain JSON")
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

#[test]
fn help_includes_export_usage() {
    let output = wax().arg("--help").output().expect("wax should execute");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8_lossy(&output.stdout).contains(
        "wax export --json <in> <out> --format xlsx|csv [--sheet N] \
[--max-cells N] [--max-bytes N] [--timeout-ms N]"
    ));
}

#[test]
fn export_reader_failure_is_one_flat_json_line_and_exits_zero() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let input = temp.path().join("values.csv");
    let output_path = temp.path().join("copy.xlsx");
    std::fs::write(&input, "one,two\n").expect("fixture should be written");

    let output = wax()
        .args(["export", "--json"])
        .arg(&input)
        .arg(&output_path)
        .args(["--format", "xlsx"])
        .output()
        .expect("wax should execute");
    let result = json_stdout(&output);

    assert_eq!(result["ok"], false);
    assert_eq!(result["code"], "unsupported");
    assert!(result["msg"].is_string());
    assert!(result.get("error").is_none());
    assert!(!output_path.exists());
}

#[test]
fn export_writer_failure_is_one_flat_json_line_and_exits_zero() {
    // Originally pinned against the pre-W4A stub; now that the writer is
    // real, force a genuine writer failure: an output directory the
    // process cannot create files in.
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("temporary directory");
    let readonly = temp.path().join("readonly");
    std::fs::create_dir(&readonly).expect("readonly directory should be created");
    std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o555))
        .expect("permissions should be set");
    let output_path = readonly.join("copy.xlsx");

    let output = wax()
        .args(["export", "--json"])
        .arg(fixture_path())
        .arg(&output_path)
        .args(["--format", "xlsx"])
        .output()
        .expect("wax should execute");
    let result = json_stdout(&output);

    assert_eq!(result["ok"], false);
    assert_eq!(result["code"], "internal");
    assert!(result["msg"].is_string());
    assert!(!output_path.exists());

    std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o755))
        .expect("permissions should be restored for cleanup");
}

#[test]
fn export_validates_sheet_range_before_writer_call() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let output_path = temp.path().join("copy.xlsx");

    let output = wax()
        .args(["export", "--json"])
        .arg(fixture_path())
        .arg(&output_path)
        .args(["--format", "xlsx", "--sheet", "99"])
        .output()
        .expect("wax should execute");
    let result = json_stdout(&output);

    assert_eq!(result["ok"], false);
    assert_eq!(result["code"], "bad_request");
    assert!(result["msg"]
        .as_str()
        .expect("message")
        .contains("sheet index 99"));
}

#[test]
fn export_rejects_invalid_format_and_sheet_as_usage_errors() {
    for arguments in [
        vec!["export", "--json", "in.xlsx", "out.xlsx", "--format", "pdf"],
        vec![
            "export", "--json", "in.xlsx", "out.xlsx", "--format", "xlsx", "--sheet", "-1",
        ],
    ] {
        let output = wax().args(arguments).output().expect("wax should execute");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
#[ignore = "requires the W4A wax-write xlsx implementation"]
fn export_success_reports_json_and_produces_a_readable_workbook() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let output_path = temp.path().join("copy.xlsx");

    let output = wax()
        .args(["export", "--json"])
        .arg(fixture_path())
        .arg(&output_path)
        .args(["--format", "xlsx"])
        .output()
        .expect("wax should execute");
    let result = json_stdout(&output);
    assert_eq!(result["ok"], true);
    assert!(result["bytes"].as_u64().expect("byte count") > 0);
    assert!(result["dropped"].is_array());

    let dumped = wax()
        .args(["dump", "--json"])
        .arg(&output_path)
        .output()
        .expect("wax should execute");
    let document = json_stdout(&dumped);
    assert_eq!(document["ok"], true);
    assert_eq!(document["sheets"][0]["cells"][0]["v"], "Hello shared");
    assert_eq!(document["sheets"][0]["merges"][0], "A3:B3");
}

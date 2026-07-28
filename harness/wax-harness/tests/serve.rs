use std::path::{Path, PathBuf};
use std::time::Duration;

use wax_harness::{run_serve_file, ServeFileConfig, ServeFileMetrics};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run(name: &str, timeout_ms: u64, export_smoke: bool) -> ServeFileMetrics {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let file = root.join(name);
    run_serve_file(ServeFileConfig {
        wax_bin: &fixture("mock-serve.js"),
        repo_root: root,
        file: &file,
        timeout: Duration::from_millis(timeout_ms),
        export_smoke,
    })
}

#[test]
fn drives_happy_path_with_out_of_order_responses_stats_and_export() {
    let result = run("happy.xlsx", 2_000, true);

    assert!(result.open_ok);
    assert_eq!(result.failure, None);
    assert!(!result.killed);
    assert_eq!(result.peak_rss_bytes, Some(52_428_800));
    let export = result.export_smoke.unwrap();
    assert!(export.ok);
    assert_eq!(export.bytes, Some(7));
    assert_eq!(export.error, None);

    let operations: Vec<_> = result
        .requests
        .iter()
        .map(|request| (request.id, request.op.as_str(), request.ok))
        .collect();
    assert_eq!(
        operations,
        vec![
            (1, "version", true),
            (2, "open", true),
            (3, "meta", true),
            (4, "window", true),
            (5, "window", true),
            (6, "window", true),
            (7, "window", true),
            (8, "window", true),
            (9, "export", true),
            (10, "stats", true),
            (11, "close", true),
        ]
    );
    assert!(result.requests.iter().all(|request| request.wall_ms >= 0.0));
}

#[test]
fn records_an_error_mid_session_and_still_collects_stats_and_closes() {
    let result = run("error.xlsx", 2_000, false);

    assert!(result.open_ok);
    assert_eq!(result.failure.as_ref().unwrap().code, "internal");
    assert_eq!(result.peak_rss_bytes, Some(52_428_800));
    let failed_window = result
        .requests
        .iter()
        .find(|request| request.op == "window" && !request.ok)
        .unwrap();
    assert_eq!(failed_window.error.as_ref().unwrap().code, "internal");
    assert!(result
        .requests
        .iter()
        .any(|request| request.op == "stats" && request.ok));
    assert!(result
        .requests
        .iter()
        .any(|request| request.op == "close" && request.ok));
}

#[test]
fn records_open_rejection_and_still_collects_process_rss() {
    let result = run("open-error.xlsx", 2_000, true);

    assert!(!result.open_ok);
    assert_eq!(result.failure.as_ref().unwrap().code, "bad_zip");
    assert_eq!(result.peak_rss_bytes, Some(52_428_800));
    assert_eq!(
        result.export_smoke.unwrap().error.unwrap().code,
        "open_failed"
    );
    assert!(result
        .requests
        .iter()
        .any(|request| request.op == "stats" && request.ok));
    assert!(!result.requests.iter().any(|request| request.op == "close"));
}

#[test]
fn records_process_death_for_every_in_flight_request() {
    let result = run("death.xlsx", 2_000, false);

    assert!(result.open_ok);
    assert_eq!(result.failure.as_ref().unwrap().code, "process_exit");
    let failures: Vec<_> = result
        .requests
        .iter()
        .filter(|request| !request.ok)
        .collect();
    assert_eq!(failures.len(), 6);
    assert!(failures
        .iter()
        .any(|request| request.error.as_ref().unwrap().code == "process_exit"));
    assert!(failures
        .iter()
        .any(|request| request.error.as_ref().unwrap().code == "client_killed"));
}

#[test]
fn kills_a_hung_server_and_records_the_client_timeout() {
    let result = run("hang.xlsx", 500, false);

    assert!(result.open_ok);
    assert!(result.killed);
    assert_eq!(result.failure.as_ref().unwrap().code, "client_timeout");
    let hung = result
        .requests
        .iter()
        .find(|request| {
            request.op == "window"
                && request
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == "client_timeout")
        })
        .unwrap();
    assert!(hung.wall_ms >= 375.0);
}

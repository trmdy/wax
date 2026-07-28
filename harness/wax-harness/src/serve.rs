use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tempfile::Builder;
use wait_timeout::ChildExt;

const PROTOCOL_VERSION: u64 = 0;
const WINDOW_ROWS: u64 = 64;
const WINDOW_COLS: u64 = 24;
const MAX_DIAGNOSTIC_BYTES: usize = 4096;

#[derive(Debug, Clone)]
pub struct ServeFileConfig<'a> {
    pub wax_bin: &'a Path,
    pub repo_root: &'a Path,
    pub file: &'a Path,
    pub timeout: Duration,
    pub export_smoke: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServeFileMetrics {
    pub open_ok: bool,
    pub requests: Vec<ServeRequestMetric>,
    pub peak_rss_bytes: Option<u64>,
    pub export_smoke: Option<ExportSmokeMetric>,
    pub failure: Option<ServeFailure>,
    pub killed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServeRequestMetric {
    pub id: u64,
    pub op: String,
    pub wall_ms: f64,
    pub ok: bool,
    pub error: Option<ServeFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServeFailure {
    pub code: String,
    pub msg: String,
}

impl ServeFailure {
    fn new(code: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            msg: msg.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSmokeMetric {
    pub ok: bool,
    pub bytes: Option<u64>,
    pub error: Option<ServeFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServeAvailability {
    Available,
    Unavailable { reason: String },
    Disabled,
}

impl ServeAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub fn status(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable { .. } => "unavailable",
            Self::Disabled => "disabled",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Unavailable { reason } => Some(reason),
            Self::Available | Self::Disabled => None,
        }
    }
}

#[derive(Debug)]
struct PendingRequest {
    op: String,
    started: Instant,
}

#[derive(Debug)]
struct CompletedResponse {
    value: Value,
}

#[derive(Debug)]
enum StreamEvent {
    Line(Result<Value, String>),
    Eof,
}

struct ServeClient {
    child: Option<Child>,
    stdin: Option<BufWriter<ChildStdin>>,
    responses: Receiver<StreamEvent>,
    stdout_reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<Vec<u8>>>,
    pending: HashMap<u64, PendingRequest>,
    completed: HashMap<u64, CompletedResponse>,
    metrics: Vec<ServeRequestMetric>,
    next_id: u64,
    timeout: Duration,
    killed: bool,
}

impl ServeClient {
    fn spawn(wax_bin: &Path, repo_root: &Path, timeout: Duration) -> Result<Self, ServeFailure> {
        let mut child = Command::new(wax_bin)
            .arg("serve")
            .current_dir(repo_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                ServeFailure::new(
                    "spawn_failed",
                    format!("failed to spawn {} serve: {error}", wax_bin.display()),
                )
            })?;
        let stdin = child
            .stdin
            .take()
            .expect("piped child stdin must be available");
        let stdout = child
            .stdout
            .take()
            .expect("piped child stdout must be available");
        let stderr = child
            .stderr
            .take()
            .expect("piped child stderr must be available");
        let (sender, responses) = mpsc::channel();
        let stdout_reader = thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let event = match line {
                    Ok(line) => StreamEvent::Line(
                        serde_json::from_str(&line)
                            .map_err(|error| format!("invalid response JSON: {error}")),
                    ),
                    Err(error) => {
                        let _ = sender.send(StreamEvent::Line(Err(format!(
                            "failed to read response: {error}"
                        ))));
                        return;
                    }
                };
                if sender.send(event).is_err() {
                    return;
                }
            }
            let _ = sender.send(StreamEvent::Eof);
        });
        let stderr_reader = thread::spawn(move || capture_diagnostic(stderr));

        Ok(Self {
            child: Some(child),
            stdin: Some(BufWriter::new(stdin)),
            responses,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(stderr_reader),
            pending: HashMap::new(),
            completed: HashMap::new(),
            metrics: Vec::new(),
            next_id: 1,
            timeout,
            killed: false,
        })
    }

    fn issue(&mut self, op: &str, build: impl FnOnce(u64) -> Value) -> Result<u64, ServeFailure> {
        let id = self.next_id;
        self.next_id += 1;
        let request = build(id);
        self.pending.insert(
            id,
            PendingRequest {
                op: op.to_owned(),
                started: Instant::now(),
            },
        );
        let write_result = self
            .stdin
            .as_mut()
            .ok_or_else(|| ServeFailure::new("stdin_closed", "serve stdin is closed"))
            .and_then(|stdin| {
                serde_json::to_writer(&mut *stdin, &request)
                    .map_err(|error| {
                        ServeFailure::new(
                            "request_encode",
                            format!("failed to encode {op} request: {error}"),
                        )
                    })
                    .and_then(|()| {
                        stdin.write_all(b"\n").map_err(|error| {
                            ServeFailure::new(
                                "request_write",
                                format!("failed to write {op} request: {error}"),
                            )
                        })
                    })
                    .and_then(|()| {
                        stdin.flush().map_err(|error| {
                            ServeFailure::new(
                                "request_write",
                                format!("failed to flush {op} request: {error}"),
                            )
                        })
                    })
            });
        if let Err(failure) = write_result {
            self.abort_pending(id, failure.clone());
            return Err(failure);
        }
        Ok(id)
    }

    fn receive(&mut self, target: u64) -> Result<Value, ServeFailure> {
        if let Some(response) = self.completed.remove(&target) {
            return Ok(response.value);
        }
        let Some(target_started) = self.pending.get(&target).map(|request| request.started) else {
            return Err(ServeFailure::new(
                "client_error",
                format!("request id {target} is not pending"),
            ));
        };
        let deadline = target_started + self.timeout;

        loop {
            let now = Instant::now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                let failure = ServeFailure::new(
                    "client_timeout",
                    format!(
                        "request id {target} exceeded {} ms",
                        self.timeout.as_millis()
                    ),
                );
                self.abort_pending(target, failure.clone());
                return Err(failure);
            };
            let event = match self.responses.recv_timeout(remaining) {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let failure = ServeFailure::new(
                        "client_timeout",
                        format!(
                            "request id {target} exceeded {} ms",
                            self.timeout.as_millis()
                        ),
                    );
                    self.abort_pending(target, failure.clone());
                    return Err(failure);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => StreamEvent::Eof,
            };
            match event {
                StreamEvent::Line(Ok(value)) => {
                    let Some(id) = value.get("id").and_then(Value::as_u64) else {
                        let failure = ServeFailure::new(
                            "protocol_error",
                            "response did not contain a usable integer id",
                        );
                        self.abort_pending(target, failure.clone());
                        return Err(failure);
                    };
                    let Some(pending) = self.pending.remove(&id) else {
                        let failure = ServeFailure::new(
                            "protocol_error",
                            format!("response id {id} was unknown or duplicated"),
                        );
                        self.abort_pending(target, failure.clone());
                        return Err(failure);
                    };
                    let error = response_failure(&value);
                    self.metrics.push(ServeRequestMetric {
                        id,
                        op: pending.op,
                        wall_ms: elapsed_ms(pending.started),
                        ok: error.is_none(),
                        error,
                    });
                    if id == target {
                        return Ok(value);
                    }
                    self.completed.insert(id, CompletedResponse { value });
                }
                StreamEvent::Line(Err(message)) => {
                    let failure = ServeFailure::new("protocol_error", message);
                    self.abort_pending(target, failure.clone());
                    return Err(failure);
                }
                StreamEvent::Eof => {
                    let failure = ServeFailure::new(
                        "process_exit",
                        format!("serve exited before responding to request id {target}"),
                    );
                    self.abort_pending(target, failure.clone());
                    return Err(failure);
                }
            }
        }
    }

    fn mark_request_failed(&mut self, id: u64, failure: ServeFailure) {
        if let Some(metric) = self.metrics.iter_mut().find(|metric| metric.id == id) {
            metric.ok = false;
            metric.error = Some(failure);
        }
    }

    fn abort_pending(&mut self, target: u64, target_failure: ServeFailure) {
        let pending = std::mem::take(&mut self.pending);
        for (id, request) in pending {
            let failure = if id == target {
                target_failure.clone()
            } else {
                ServeFailure::new(
                    "client_killed",
                    format!(
                        "request id {id} was still in flight when the serve process was killed"
                    ),
                )
            };
            self.metrics.push(ServeRequestMetric {
                id,
                op: request.op,
                wall_ms: elapsed_ms(request.started),
                ok: false,
                error: Some(failure),
            });
        }
        self.completed.clear();
        self.kill();
    }

    fn kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
                self.killed = true;
            }
        }
        self.stdin.take();
    }

    fn shutdown(&mut self) -> Option<ServeFailure> {
        self.stdin.take();
        let status = if let Some(child) = self.child.as_mut() {
            match child.wait_timeout(self.timeout) {
                Ok(Some(status)) => Some(status),
                Ok(None) => {
                    let _ = child.kill();
                    self.killed = true;
                    let _ = child.wait();
                    None
                }
                Err(_) => {
                    let _ = child.kill();
                    self.killed = true;
                    let _ = child.wait();
                    None
                }
            }
        } else {
            None
        };
        self.child.take();
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        let diagnostic = self
            .stderr_reader
            .take()
            .and_then(|reader| reader.join().ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned())
            .unwrap_or_default();

        match status {
            Some(status) if status.success() => None,
            Some(status) => {
                let suffix = if diagnostic.is_empty() {
                    String::new()
                } else {
                    format!(": {diagnostic}")
                };
                Some(ServeFailure::new(
                    process_exit_code(&status),
                    format!("serve exited with {status}{suffix}"),
                ))
            }
            None => Some(ServeFailure::new(
                "eof_timeout",
                format!(
                    "serve did not exit within {} ms after stdin EOF and was killed",
                    self.timeout.as_millis()
                ),
            )),
        }
    }
}

impl Drop for ServeClient {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn detect_serve(wax_bin: &Path, repo_root: &Path, timeout: Duration) -> ServeAvailability {
    let mut child = match Command::new(wax_bin)
        .args(["serve", "--help"])
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return ServeAvailability::Unavailable {
                reason: format!("serve probe could not spawn {}: {error}", wax_bin.display()),
            };
        }
    };
    let stdout = child.stdout.take().expect("piped stdout must be available");
    let stderr = child.stderr.take().expect("piped stderr must be available");
    let stdout_reader = thread::spawn(move || capture_diagnostic(stdout));
    let stderr_reader = thread::spawn(move || capture_diagnostic(stderr));
    let wait = child.wait_timeout(timeout);
    let status = match wait {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return ServeAvailability::Available;
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return ServeAvailability::Available;
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    if status.success() {
        return ServeAvailability::Available;
    }
    let diagnostic = format!(
        "{}\n{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    let diagnostic_lower = diagnostic.to_ascii_lowercase();
    if diagnostic_lower.contains("unrecognized subcommand")
        || diagnostic_lower.contains("unknown subcommand")
        || diagnostic_lower.contains("unexpected argument 'serve'")
        || diagnostic_lower.contains("invalid command")
    {
        let reason = diagnostic
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("wax binary does not support serve")
            .trim()
            .to_owned();
        ServeAvailability::Unavailable { reason }
    } else {
        // A supported-but-broken serve command belongs in the per-file failure
        // data. Only a recognizable CLI "unknown command" response is n/a.
        ServeAvailability::Available
    }
}

pub fn run_serve_file(config: ServeFileConfig<'_>) -> ServeFileMetrics {
    let mut output = ServeFileMetrics::default();
    let mut client = match ServeClient::spawn(config.wax_bin, config.repo_root, config.timeout) {
        Ok(client) => client,
        Err(failure) => {
            output.failure = Some(failure);
            return output;
        }
    };

    let version_id = match client.issue("version", version_request) {
        Ok(id) => id,
        Err(failure) => return finish_failure(output, client, failure),
    };
    let version = match client.receive(version_id) {
        Ok(response) => response,
        Err(failure) => return finish_failure(output, client, failure),
    };
    if let Err(failure) = validate_version(&version) {
        client.mark_request_failed(version_id, failure.clone());
        return finish_failure(output, client, failure);
    }

    let open_id = match client.issue("open", |id| {
        open_request(
            id,
            &config.file.to_string_lossy(),
            config.timeout.as_millis().min(u64::MAX as u128) as u64,
        )
    }) {
        Ok(id) => id,
        Err(failure) => return finish_failure(output, client, failure),
    };
    let open = match client.receive(open_id) {
        Ok(response) => response,
        Err(failure) => return finish_failure(output, client, failure),
    };
    if let Some(failure) = response_failure(&open) {
        remember_failure(&mut output.failure, failure);
        output.export_smoke = config.export_smoke.then(|| ExportSmokeMetric {
            ok: false,
            bytes: None,
            error: Some(ServeFailure::new(
                "open_failed",
                "export smoke was skipped because open failed",
            )),
        });
        if let Err(failure) = collect_stats(&mut client, &mut output) {
            return finish_failure(output, client, failure);
        }
        return finish(output, client);
    }
    let (handle, sheets) = match validate_open(&open) {
        Ok(fields) => fields,
        Err(failure) => {
            client.mark_request_failed(open_id, failure.clone());
            return finish_failure(output, client, failure);
        }
    };
    output.open_ok = true;

    let meta_id = match client.issue("meta", |id| meta_request(id, &handle)) {
        Ok(id) => id,
        Err(failure) => return finish_failure(output, client, failure),
    };
    let mut windows = Vec::new();
    if let Some((rows, cols)) = sheets.first().copied() {
        for (r0, c0) in window_offsets(rows, cols) {
            let id = match client.issue("window", |id| window_request(id, &handle, 0, r0, c0)) {
                Ok(id) => id,
                Err(failure) => return finish_failure(output, client, failure),
            };
            windows.push((id, r0, c0));
        }
    }

    match client.receive(meta_id) {
        Ok(response) => {
            if let Err(failure) = validate_meta(&response) {
                client.mark_request_failed(meta_id, failure.clone());
                remember_failure(&mut output.failure, failure);
            }
        }
        Err(failure) => return finish_failure(output, client, failure),
    }
    for (id, r0, c0) in windows {
        match client.receive(id) {
            Ok(response) => {
                if let Err(failure) = validate_window(&response, r0, c0) {
                    client.mark_request_failed(id, failure.clone());
                    remember_failure(&mut output.failure, failure);
                }
            }
            Err(failure) => return finish_failure(output, client, failure),
        }
    }

    if config.export_smoke {
        let export = run_export_smoke(&mut client, &handle);
        if let Some(failure) = export.error.clone() {
            remember_failure(&mut output.failure, failure);
        }
        output.export_smoke = Some(export);
    }

    if let Err(failure) = collect_stats(&mut client, &mut output) {
        return finish_failure(output, client, failure);
    }

    let close_id = match client.issue("close", |id| close_request(id, &handle)) {
        Ok(id) => id,
        Err(failure) => return finish_failure(output, client, failure),
    };
    match client.receive(close_id) {
        Ok(response) => {
            if let Some(failure) = response_failure(&response) {
                remember_failure(&mut output.failure, failure);
            }
        }
        Err(failure) => return finish_failure(output, client, failure),
    }

    finish(output, client)
}

fn run_export_smoke(client: &mut ServeClient, handle: &str) -> ExportSmokeMetric {
    let directory = match Builder::new().prefix("wax-harness-export-").tempdir() {
        Ok(directory) => directory,
        Err(error) => {
            return ExportSmokeMetric {
                ok: false,
                bytes: None,
                error: Some(ServeFailure::new(
                    "tempdir_failed",
                    format!("failed to create export smoke temp dir: {error}"),
                )),
            };
        }
    };
    let output_path = directory.path().join("export.csv");
    let export_id = match client.issue("export", |id| {
        export_request(id, handle, &output_path.to_string_lossy())
    }) {
        Ok(id) => id,
        Err(failure) => {
            return ExportSmokeMetric {
                ok: false,
                bytes: None,
                error: Some(failure),
            };
        }
    };
    let response = match client.receive(export_id) {
        Ok(response) => response,
        Err(failure) => {
            return ExportSmokeMetric {
                ok: false,
                bytes: None,
                error: Some(failure),
            };
        }
    };
    if let Some(failure) = response_failure(&response) {
        return ExportSmokeMetric {
            ok: false,
            bytes: None,
            error: Some(failure),
        };
    }
    let response_bytes = match response.get("bytes").and_then(Value::as_u64) {
        Some(bytes) => bytes,
        None => {
            let failure = ServeFailure::new(
                "protocol_error",
                "export response had no integer bytes field",
            );
            client.mark_request_failed(export_id, failure.clone());
            return ExportSmokeMetric {
                ok: false,
                bytes: None,
                error: Some(failure),
            };
        }
    };
    match fs::metadata(&output_path) {
        Ok(metadata) if metadata.len() > 0 && response_bytes > 0 => ExportSmokeMetric {
            ok: true,
            bytes: Some(metadata.len()),
            error: None,
        },
        Ok(metadata) => {
            let failure = ServeFailure::new(
                "empty_export",
                format!(
                    "export response reported {response_bytes} bytes and output contained {} bytes",
                    metadata.len()
                ),
            );
            client.mark_request_failed(export_id, failure.clone());
            ExportSmokeMetric {
                ok: false,
                bytes: Some(metadata.len()),
                error: Some(failure),
            }
        }
        Err(error) => {
            let failure = ServeFailure::new(
                "missing_export",
                format!(
                    "export output {} was not readable: {error}",
                    output_path.display()
                ),
            );
            client.mark_request_failed(export_id, failure.clone());
            ExportSmokeMetric {
                ok: false,
                bytes: None,
                error: Some(failure),
            }
        }
    }
}

fn collect_stats(
    client: &mut ServeClient,
    output: &mut ServeFileMetrics,
) -> Result<(), ServeFailure> {
    let stats_id = client.issue("stats", stats_request)?;
    let response = client.receive(stats_id)?;
    match validate_stats(&response) {
        Ok(peak_rss_bytes) => output.peak_rss_bytes = Some(peak_rss_bytes),
        Err(failure) => {
            client.mark_request_failed(stats_id, failure.clone());
            remember_failure(&mut output.failure, failure);
        }
    }
    Ok(())
}

fn finish_failure(
    mut output: ServeFileMetrics,
    client: ServeClient,
    failure: ServeFailure,
) -> ServeFileMetrics {
    remember_failure(&mut output.failure, failure);
    finish(output, client)
}

fn finish(mut output: ServeFileMetrics, mut client: ServeClient) -> ServeFileMetrics {
    if let Some(failure) = client.shutdown() {
        remember_failure(&mut output.failure, failure);
    }
    client.metrics.sort_by_key(|metric| metric.id);
    output.requests = std::mem::take(&mut client.metrics);
    output.killed = client.killed;
    output
}

fn remember_failure(target: &mut Option<ServeFailure>, failure: ServeFailure) {
    if target.is_none() {
        *target = Some(failure);
    }
}

pub fn window_offsets(rows: u64, cols: u64) -> Vec<(u64, u64)> {
    let last_row = rows.saturating_sub(WINDOW_ROWS);
    let last_col = cols.saturating_sub(WINDOW_COLS);
    let candidates = [
        (0, 0),
        (0, last_col),
        (last_row, 0),
        (last_row, last_col),
        (last_row / 2, last_col / 2),
    ];
    let mut offsets = Vec::with_capacity(candidates.len());
    for offset in candidates {
        if !offsets.contains(&offset) {
            offsets.push(offset);
        }
    }
    offsets
}

fn version_request(id: u64) -> Value {
    json!({"id": id, "op": "version"})
}

fn open_request(id: u64, path: &str, timeout_ms: u64) -> Value {
    json!({
        "id": id,
        "op": "open",
        "path": path,
        "timeoutMs": timeout_ms
    })
}

fn meta_request(id: u64, handle: &str) -> Value {
    json!({"id": id, "op": "meta", "handle": handle})
}

fn window_request(id: u64, handle: &str, sheet: u64, r0: u64, c0: u64) -> Value {
    json!({
        "id": id,
        "op": "window",
        "handle": handle,
        "sheet": sheet,
        "r0": r0,
        "c0": c0,
        "nr": WINDOW_ROWS,
        "nc": WINDOW_COLS
    })
}

fn export_request(id: u64, handle: &str, out: &str) -> Value {
    json!({
        "id": id,
        "op": "export",
        "handle": handle,
        "format": "csv",
        "out": out,
        "sheet": 0
    })
}

fn stats_request(id: u64) -> Value {
    json!({"id": id, "op": "stats"})
}

fn close_request(id: u64, handle: &str) -> Value {
    json!({"id": id, "op": "close", "handle": handle})
}

fn validate_version(response: &Value) -> Result<(), ServeFailure> {
    ensure_success(response, "version")?;
    if response.get("proto").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
        return Err(ServeFailure::new(
            "protocol_error",
            "version response did not advertise proto 0",
        ));
    }
    if response
        .get("version")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(ServeFailure::new(
            "protocol_error",
            "version response had no non-empty version",
        ));
    }
    Ok(())
}

fn validate_open(response: &Value) -> Result<(String, Vec<(u64, u64)>), ServeFailure> {
    ensure_success(response, "open")?;
    if response.get("proto").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
        return Err(ServeFailure::new(
            "protocol_error",
            "open response did not advertise proto 0",
        ));
    }
    let handle = response
        .get("handle")
        .and_then(Value::as_str)
        .filter(|handle| !handle.is_empty())
        .ok_or_else(|| ServeFailure::new("protocol_error", "open response had no handle"))?
        .to_owned();
    let sheets = parse_sheet_extents(response, "open")?;
    Ok((handle, sheets))
}

fn validate_meta(response: &Value) -> Result<(), ServeFailure> {
    ensure_success(response, "meta")?;
    parse_sheet_extents(response, "meta").map(|_| ())
}

fn parse_sheet_extents(response: &Value, op: &str) -> Result<Vec<(u64, u64)>, ServeFailure> {
    let sheets = response
        .get("sheets")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ServeFailure::new(
                "protocol_error",
                format!("{op} response had no sheets array"),
            )
        })?;
    sheets
        .iter()
        .enumerate()
        .map(|(index, sheet)| {
            let rows = sheet.get("rows").and_then(Value::as_u64).ok_or_else(|| {
                ServeFailure::new(
                    "protocol_error",
                    format!("{op} response sheet {index} had no integer rows"),
                )
            })?;
            let cols = sheet.get("cols").and_then(Value::as_u64).ok_or_else(|| {
                ServeFailure::new(
                    "protocol_error",
                    format!("{op} response sheet {index} had no integer cols"),
                )
            })?;
            Ok((rows, cols))
        })
        .collect()
}

fn validate_window(
    response: &Value,
    requested_r0: u64,
    requested_c0: u64,
) -> Result<(), ServeFailure> {
    ensure_success(response, "window")?;
    if response.get("sheet").and_then(Value::as_u64) != Some(0)
        || response.get("r0").and_then(Value::as_u64) != Some(requested_r0)
        || response.get("c0").and_then(Value::as_u64) != Some(requested_c0)
    {
        return Err(ServeFailure::new(
            "protocol_error",
            "window response did not echo sheet/r0/c0",
        ));
    }
    let nr = response
        .get("nr")
        .and_then(Value::as_u64)
        .ok_or_else(|| ServeFailure::new("protocol_error", "window response had no integer nr"))?;
    let nc = response
        .get("nc")
        .and_then(Value::as_u64)
        .ok_or_else(|| ServeFailure::new("protocol_error", "window response had no integer nc"))?;
    if nr > WINDOW_ROWS || nc > WINDOW_COLS {
        return Err(ServeFailure::new(
            "protocol_error",
            "window response exceeded the requested extent",
        ));
    }
    let rows = response
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| ServeFailure::new("protocol_error", "window response had no rows array"))?;
    if rows.len() as u64 != nr
        || rows
            .iter()
            .any(|row| row.as_array().map(|cells| cells.len() as u64) != Some(nc))
    {
        return Err(ServeFailure::new(
            "protocol_error",
            "window response row shape did not match nr/nc",
        ));
    }
    if !response
        .get("merges")
        .is_some_and(|merges| merges.is_array())
    {
        return Err(ServeFailure::new(
            "protocol_error",
            "window response had no merges array",
        ));
    }
    Ok(())
}

fn validate_stats(response: &Value) -> Result<u64, ServeFailure> {
    ensure_success(response, "stats")?;
    response
        .get("peakRssBytes")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            ServeFailure::new(
                "protocol_error",
                "stats response had no integer peakRssBytes",
            )
        })
}

fn ensure_success(response: &Value, op: &str) -> Result<(), ServeFailure> {
    if let Some(failure) = response_failure(response) {
        Err(failure)
    } else if response.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(ServeFailure::new(
            "protocol_error",
            format!("{op} response had no boolean ok"),
        ))
    }
}

fn response_failure(response: &Value) -> Option<ServeFailure> {
    match response.get("ok").and_then(Value::as_bool) {
        Some(true) => None,
        Some(false) => Some(ServeFailure::new(
            response
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("protocol_error"),
            response
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("serve returned an error"),
        )),
        None => Some(ServeFailure::new(
            "protocol_error",
            "response had no boolean ok",
        )),
    }
}

fn capture_diagnostic(mut reader: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let remaining = MAX_DIAGNOSTIC_BYTES.saturating_sub(bytes.len());
        bytes.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    bytes
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn process_exit_code(status: &ExitStatus) -> &'static str {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() == Some(9) {
            return "oom_or_crash";
        }
        if status.signal().is_some() {
            return "process_crash";
        }
    }
    "process_exit"
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        close_request, export_request, meta_request, open_request, stats_request, version_request,
        window_offsets, window_request,
    };

    #[test]
    fn request_builders_match_protocol_v0() {
        assert_eq!(version_request(1), json!({"id": 1, "op": "version"}));
        assert_eq!(
            open_request(2, "/tmp/example.xlsx", 30_000),
            json!({
                "id": 2,
                "op": "open",
                "path": "/tmp/example.xlsx",
                "timeoutMs": 30_000
            })
        );
        assert_eq!(
            meta_request(3, "h1"),
            json!({"id": 3, "op": "meta", "handle": "h1"})
        );
        assert_eq!(
            window_request(4, "h1", 0, 64, 24),
            json!({
                "id": 4,
                "op": "window",
                "handle": "h1",
                "sheet": 0,
                "r0": 64,
                "c0": 24,
                "nr": 64,
                "nc": 24
            })
        );
        assert_eq!(
            export_request(5, "h1", "/tmp/out.csv"),
            json!({
                "id": 5,
                "op": "export",
                "handle": "h1",
                "format": "csv",
                "out": "/tmp/out.csv",
                "sheet": 0
            })
        );
        assert_eq!(stats_request(6), json!({"id": 6, "op": "stats"}));
        assert_eq!(
            close_request(7, "h1"),
            json!({"id": 7, "op": "close", "handle": "h1"})
        );
    }

    #[test]
    fn window_offsets_cover_corners_and_center_without_duplicates() {
        assert_eq!(
            window_offsets(128, 48),
            vec![(0, 0), (0, 24), (64, 0), (64, 24), (32, 12)]
        );
        assert_eq!(window_offsets(64, 48), vec![(0, 0), (0, 24), (0, 12)]);
        assert_eq!(window_offsets(128, 24), vec![(0, 0), (64, 0), (32, 0)]);
        assert_eq!(window_offsets(10, 10), vec![(0, 0)]);
        assert_eq!(window_offsets(0, 0), vec![(0, 0)]);
    }
}

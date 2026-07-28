use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use wait_timeout::ChildExt;

use crate::compare::{cells_have_equal_values, CountMetric};
use crate::model::{Cell, DumpDocument, ExpectedDump, Tool};

const MAX_CAPTURE_BYTES: u64 = 256 * 1024 * 1024;
pub const SOFFICE_SUBSET_FILES: usize = 200;
pub const SOFFICE_MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
pub const SOFFICE_SUBSET_SEED: u64 = 0x5741_582d_5734_4235;
const SOFFICE_MIN_TIMEOUT: Duration = Duration::from_secs(60);
const APP_BUNDLE_SOFFICE: &str = "/Applications/LibreOffice.app/Contents/MacOS/soffice";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SofficeAvailability {
    Disabled,
    Unavailable { reason: String },
    Available { executable: PathBuf },
}

impl SofficeAvailability {
    pub fn status(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Unavailable { .. } => "unavailable",
            Self::Available { .. } => "available",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Disabled => Some("soffice disabled"),
            Self::Unavailable { reason } => Some(reason),
            Self::Available { .. } => None,
        }
    }

    fn executable(&self) -> Option<&Path> {
        match self {
            Self::Available { executable } => Some(executable),
            Self::Disabled | Self::Unavailable { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundTripFileMetrics {
    pub status: String,
    pub value_match: CountMetric,
    pub display_match: CountMetric,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merge_defects: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub structure_defects: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle_open: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soffice_open: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RoundTripFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle_error: Option<RoundTripFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soffice_error: Option<RoundTripFailure>,
}

impl RoundTripFileMetrics {
    pub fn skipped_truncated() -> Self {
        Self {
            status: "skippedTruncated".to_owned(),
            ..Self::default()
        }
    }

    pub fn is_unavailable(&self) -> bool {
        self.status == "xlsxExportUnavailable"
    }

    pub fn was_attempted(&self) -> bool {
        self.status != "skippedTruncated"
    }

    pub fn is_clean(&self) -> bool {
        self.status == "clean"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundTripFailure {
    pub stage: String,
    pub code: String,
    pub msg: String,
}

impl RoundTripFailure {
    fn new(stage: &str, code: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            stage: stage.to_owned(),
            code: code.into(),
            msg: msg.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoundTripComparison {
    pub value_match: CountMetric,
    pub display_match: CountMetric,
    pub merge_defects: Vec<String>,
    pub structure_defects: Vec<String>,
    pub extra_target_cells: u64,
}

impl RoundTripComparison {
    fn is_clean(&self) -> bool {
        self.value_match.matched == self.value_match.total
            && self.display_match.matched == self.display_match.total
            && self.merge_defects.is_empty()
            && self.structure_defects.is_empty()
            && self.extra_target_cells == 0
    }
}

pub struct RoundTripFileConfig<'a> {
    pub wax_bin: &'a Path,
    pub node_bin: &'a Path,
    pub oracle_script: &'a Path,
    pub repo_root: &'a Path,
    pub source_path: &'a Path,
    pub max_cells: u64,
    pub timeout: Duration,
    pub run_soffice: bool,
    pub soffice: &'a SofficeAvailability,
}

#[derive(Debug)]
struct CapturedProcess {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    output_too_large: bool,
}

#[derive(Debug, Deserialize)]
struct ExportResponse {
    ok: bool,
    #[serde(default)]
    bytes: Option<u64>,
    #[serde(default)]
    dropped: Vec<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    msg: Option<String>,
}

enum ExportResult {
    Exported,
    Unavailable(RoundTripFailure),
    Failed(RoundTripFailure),
}

pub fn detect_soffice(enabled: bool, explicit: Option<&Path>) -> SofficeAvailability {
    if !enabled {
        return SofficeAvailability::Disabled;
    }

    if let Some(executable) = explicit {
        return if is_executable_file(executable) {
            SofficeAvailability::Available {
                executable: executable.to_path_buf(),
            }
        } else {
            SofficeAvailability::Unavailable {
                reason: format!(
                    "configured soffice is unavailable: {}",
                    executable.display()
                ),
            }
        };
    }

    if let Some(executable) = env::var_os("PATH")
        .as_deref()
        .and_then(|path| find_on_path(path, "soffice"))
    {
        return SofficeAvailability::Available { executable };
    }

    let app_bundle = PathBuf::from(APP_BUNDLE_SOFFICE);
    if is_executable_file(&app_bundle) {
        SofficeAvailability::Available {
            executable: app_bundle,
        }
    } else {
        SofficeAvailability::Unavailable {
            reason: "soffice unavailable".to_owned(),
        }
    }
}

pub fn find_on_path(path: &std::ffi::OsStr, executable: &str) -> Option<PathBuf> {
    env::split_paths(path)
        .map(|directory| directory.join(executable))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub fn select_soffice_subset<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a str, u64)>,
    limit: usize,
    max_source_bytes: u64,
    seed: u64,
) -> HashSet<String> {
    let mut by_extension = BTreeMap::<String, Vec<(u64, String)>>::new();
    for (id, extension, bytes) in entries {
        if bytes > max_source_bytes {
            continue;
        }
        by_extension
            .entry(extension.to_ascii_lowercase())
            .or_default()
            .push((seeded_hash(seed, id), id.to_owned()));
    }

    let mut queues: Vec<_> = by_extension
        .into_values()
        .map(|mut entries| {
            entries.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
            VecDeque::from(entries)
        })
        .collect();
    let mut selected = HashSet::with_capacity(limit);
    while selected.len() < limit {
        let mut progressed = false;
        for queue in &mut queues {
            if selected.len() == limit {
                break;
            }
            if let Some((_, id)) = queue.pop_front() {
                selected.insert(id);
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    selected
}

fn seeded_hash(seed: u64, value: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325 ^ seed;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub fn compare_round_trip(source: &DumpDocument, target: &DumpDocument) -> RoundTripComparison {
    let source_cells = indexed_cells(source);
    let target_cells = indexed_cells(target);
    let mut comparison = RoundTripComparison::default();

    for (coordinate, source_cell) in &source_cells {
        let target_cell = target_cells.get(coordinate).copied();
        comparison.value_match.total += 1;
        comparison.display_match.total += 1;
        if cells_have_equal_values(Some(source_cell), target_cell) {
            comparison.value_match.matched += 1;
        }
        if target_cell.is_some_and(|target_cell| source_cell.d == target_cell.d) {
            comparison.display_match.matched += 1;
        }
    }
    comparison.extra_target_cells = target_cells
        .keys()
        .filter(|coordinate| !source_cells.contains_key(coordinate))
        .count() as u64;

    if source.sheets.len() != target.sheets.len() {
        comparison.structure_defects.push(format!(
            "sheet count differs: {} -> {}",
            source.sheets.len(),
            target.sheets.len()
        ));
    }
    let sheet_count = source.sheets.len().max(target.sheets.len());
    for index in 0..sheet_count {
        let source_sheet = source.sheets.get(index);
        let target_sheet = target.sheets.get(index);
        match (source_sheet, target_sheet) {
            (Some(source_sheet), Some(target_sheet)) => {
                if source_sheet.name != target_sheet.name {
                    comparison.structure_defects.push(format!(
                        "sheet {index} name differs: {:?} -> {:?}",
                        source_sheet.name, target_sheet.name
                    ));
                }
                let source_merges: BTreeSet<_> = source_sheet.merges.iter().collect();
                let target_merges: BTreeSet<_> = target_sheet.merges.iter().collect();
                if source_merges != target_merges {
                    comparison.merge_defects.push(format!(
                        "sheet {index} ({}) merge set differs",
                        source_sheet.name
                    ));
                }
            }
            (Some(source_sheet), None) => comparison
                .merge_defects
                .push(format!("sheet {index} ({}) is missing", source_sheet.name)),
            (None, Some(target_sheet)) => comparison
                .merge_defects
                .push(format!("extra sheet {index} ({})", target_sheet.name)),
            (None, None) => {}
        }
    }
    if comparison.extra_target_cells != 0 {
        comparison.structure_defects.push(format!(
            "{} extra target cells",
            comparison.extra_target_cells
        ));
    }
    comparison
}

fn indexed_cells(document: &DumpDocument) -> BTreeMap<(usize, u64, u64), &Cell> {
    let mut cells = BTreeMap::new();
    for sheet in &document.sheets {
        for cell in &sheet.cells {
            cells.insert((sheet.index, cell.r, cell.c), cell);
        }
    }
    cells
}

pub fn run_round_trip(
    source: &DumpDocument,
    config: RoundTripFileConfig<'_>,
) -> RoundTripFileMetrics {
    if source.truncated {
        return RoundTripFileMetrics::skipped_truncated();
    }

    let temporary = match tempdir() {
        Ok(temporary) => temporary,
        Err(error) => {
            return failed(
                "temporary",
                "io",
                format!("failed to create round-trip directory: {error}"),
            );
        }
    };
    let exported = temporary.path().join("round-trip.xlsx");
    match export_xlsx(&config, &exported) {
        ExportResult::Exported => {}
        ExportResult::Unavailable(error) => {
            return RoundTripFileMetrics {
                status: "xlsxExportUnavailable".to_owned(),
                error: Some(error),
                ..RoundTripFileMetrics::default()
            };
        }
        ExportResult::Failed(error) => {
            return RoundTripFileMetrics {
                status: "failed".to_owned(),
                error: Some(error),
                ..RoundTripFileMetrics::default()
            };
        }
    }

    let oracle = invoke_dump(
        Tool::Sheetjs,
        config.node_bin,
        &[
            config.oracle_script.as_os_str().to_owned(),
            exported.as_os_str().to_owned(),
            "--max-cells".into(),
            config.max_cells.to_string().into(),
            "--timeout-ms".into(),
            config.timeout.as_millis().to_string().into(),
        ],
        config.repo_root,
        config.timeout,
        "oracleReadBack",
    );
    let (oracle_open, oracle_error) = match oracle {
        Ok(document) if document.ok => (Some(true), None),
        Ok(document) => (
            Some(false),
            Some(document_failure("oracleReadBack", &document)),
        ),
        Err(error) => (Some(false), Some(error)),
    };

    let (soffice_open, soffice_error) = if config.run_soffice {
        match config.soffice.executable() {
            Some(executable) => match run_soffice(
                executable,
                &exported,
                config.timeout.max(SOFFICE_MIN_TIMEOUT),
            ) {
                Ok(open) => (Some(open), None),
                Err(error) => (Some(false), Some(error)),
            },
            None => (None, None),
        }
    } else {
        (None, None)
    };

    let target = invoke_dump(
        Tool::Wax,
        config.wax_bin,
        &[
            "dump".into(),
            "--json".into(),
            exported.as_os_str().to_owned(),
            "--max-cells".into(),
            config.max_cells.to_string().into(),
            "--timeout-ms".into(),
            config.timeout.as_millis().to_string().into(),
        ],
        config.repo_root,
        config.timeout,
        "waxReadBack",
    );
    let target = match target {
        Ok(target) if target.ok => target,
        Ok(target) => {
            let mut result = failed_with(document_failure("waxReadBack", &target));
            result.oracle_open = oracle_open;
            result.oracle_error = oracle_error;
            result.soffice_open = soffice_open;
            result.soffice_error = soffice_error;
            return result;
        }
        Err(error) => {
            let mut result = failed_with(error);
            result.oracle_open = oracle_open;
            result.oracle_error = oracle_error;
            result.soffice_open = soffice_open;
            result.soffice_error = soffice_error;
            return result;
        }
    };

    let comparison = compare_round_trip(source, &target);
    RoundTripFileMetrics {
        status: if comparison.is_clean() {
            "clean".to_owned()
        } else {
            "defect".to_owned()
        },
        value_match: comparison.value_match,
        display_match: comparison.display_match,
        merge_defects: comparison.merge_defects,
        structure_defects: comparison.structure_defects,
        oracle_open,
        soffice_open,
        error: None,
        oracle_error,
        soffice_error,
    }
}

fn export_xlsx(config: &RoundTripFileConfig<'_>, output: &Path) -> ExportResult {
    let arguments = [
        "export".into(),
        "--json".into(),
        config.source_path.as_os_str().to_owned(),
        output.as_os_str().to_owned(),
        "--format".into(),
        "xlsx".into(),
        "--max-cells".into(),
        config.max_cells.to_string().into(),
        "--timeout-ms".into(),
        config.timeout.as_millis().to_string().into(),
    ];
    let captured = match run_captured(config.wax_bin, &arguments, config.repo_root, config.timeout)
    {
        Ok(captured) => captured,
        Err(error) => return ExportResult::Failed(error.with_stage("export")),
    };
    if captured.output_too_large {
        return ExportResult::Failed(RoundTripFailure::new(
            "export",
            "output_too_large",
            format!("process output exceeded {MAX_CAPTURE_BYTES} bytes"),
        ));
    }
    if !captured.status.success() {
        let stderr = diagnostic(&captured.stderr);
        let error = RoundTripFailure::new(
            "export",
            "process_exit",
            if stderr.is_empty() {
                format!("process exited with {}", captured.status)
            } else {
                format!("process exited with {}: {stderr}", captured.status)
            },
        );
        return if export_cli_is_unavailable(&stderr) {
            ExportResult::Unavailable(error)
        } else {
            ExportResult::Failed(error)
        };
    }

    let response: ExportResponse = match serde_json::from_slice(&captured.stdout) {
        Ok(response) => response,
        Err(error) => {
            return ExportResult::Failed(RoundTripFailure::new(
                "export",
                "schema_violation",
                format!("invalid export JSON: {error}"),
            ));
        }
    };
    let _ = response.dropped;
    if !response.ok {
        let code = response.code.unwrap_or_else(|| "internal".to_owned());
        let error = RoundTripFailure::new(
            "export",
            &code,
            response
                .msg
                .unwrap_or_else(|| "xlsx export failed without a message".to_owned()),
        );
        return if code == "internal" {
            ExportResult::Unavailable(error)
        } else {
            ExportResult::Failed(error)
        };
    }
    if response.bytes.is_none() {
        return ExportResult::Failed(RoundTripFailure::new(
            "export",
            "schema_violation",
            "successful export response omitted bytes",
        ));
    }
    match fs::metadata(output) {
        Ok(metadata) if metadata.is_file() && metadata.len() > 0 => ExportResult::Exported,
        Ok(_) => ExportResult::Failed(RoundTripFailure::new(
            "export",
            "empty_output",
            "xlsx export did not create a non-empty file",
        )),
        Err(error) => ExportResult::Failed(RoundTripFailure::new(
            "export",
            "missing_output",
            format!("xlsx export output is unavailable: {error}"),
        )),
    }
}

fn export_cli_is_unavailable(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    (stderr.contains("unrecognized") || stderr.contains("unknown"))
        && (stderr.contains("subcommand") || stderr.contains("command"))
        && stderr.contains("export")
}

fn invoke_dump(
    tool: Tool,
    executable: &Path,
    arguments: &[std::ffi::OsString],
    repo_root: &Path,
    timeout: Duration,
    stage: &str,
) -> Result<DumpDocument, RoundTripFailure> {
    let captured = run_captured(executable, arguments, repo_root, timeout)
        .map_err(|error| error.with_stage(stage))?;
    if captured.output_too_large {
        return Err(RoundTripFailure::new(
            stage,
            "output_too_large",
            format!("process output exceeded {MAX_CAPTURE_BYTES} bytes"),
        ));
    }
    if !captured.status.success() {
        let stderr = diagnostic(&captured.stderr);
        return Err(RoundTripFailure::new(
            stage,
            "process_exit",
            if stderr.is_empty() {
                format!("process exited with {}", captured.status)
            } else {
                format!("process exited with {}: {stderr}", captured.status)
            },
        ));
    }
    DumpDocument::parse(&captured.stdout, ExpectedDump { tool, sha256: None })
        .map_err(|error| RoundTripFailure::new(stage, "schema_violation", error.to_string()))
}

fn run_captured(
    executable: &Path,
    arguments: &[std::ffi::OsString],
    current_dir: &Path,
    timeout: Duration,
) -> Result<CapturedProcess, RoundTripFailure> {
    let mut child = Command::new(executable)
        .args(arguments)
        .current_dir(current_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            RoundTripFailure::new(
                "process",
                "spawn_failed",
                format!("failed to spawn {}: {error}", executable.display()),
            )
        })?;
    let stdout = child.stdout.take().expect("piped stdout must be present");
    let stderr = child.stderr.take().expect("piped stderr must be present");
    let stdout_reader = thread::spawn(move || capture(stdout));
    let stderr_reader = thread::spawn(move || capture(stderr));

    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(RoundTripFailure::new(
                "process",
                "timeout",
                format!("process exceeded {} ms", timeout.as_millis()),
            ));
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(RoundTripFailure::new(
                "process",
                "wait_failed",
                format!("failed while waiting for process: {error}"),
            ));
        }
    };
    let (stdout, stdout_too_large) = join_capture(stdout_reader);
    let (stderr, stderr_too_large) = join_capture(stderr_reader);
    Ok(CapturedProcess {
        status,
        stdout,
        stderr,
        output_too_large: stdout_too_large || stderr_too_large,
    })
}

fn run_soffice(
    executable: &Path,
    input: &Path,
    timeout: Duration,
) -> Result<bool, RoundTripFailure> {
    let temporary = tempdir().map_err(|error| {
        RoundTripFailure::new(
            "soffice",
            "io",
            format!("failed to create soffice directory: {error}"),
        )
    })?;
    let output_dir = temporary.path().join("out");
    let profile_dir = temporary.path().join("profile");
    fs::create_dir(&output_dir).map_err(|error| {
        RoundTripFailure::new(
            "soffice",
            "io",
            format!("failed to create soffice output directory: {error}"),
        )
    })?;
    let profile_url = format!("file://{}", profile_dir.display());
    let mut child = Command::new(executable)
        .arg("--headless")
        .arg(format!("-env:UserInstallation={profile_url}"))
        .arg("--convert-to")
        .arg("xlsx")
        .arg("--outdir")
        .arg(&output_dir)
        .arg(input)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            RoundTripFailure::new(
                "soffice",
                "spawn_failed",
                format!("failed to spawn {}: {error}", executable.display()),
            )
        })?;
    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RoundTripFailure::new(
                "soffice",
                "timeout",
                format!("soffice exceeded {} ms", timeout.as_millis()),
            ));
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RoundTripFailure::new(
                "soffice",
                "wait_failed",
                format!("failed while waiting for soffice: {error}"),
            ));
        }
    };
    if !status.success() {
        return Ok(false);
    }
    let output = output_dir.join(
        input
            .file_name()
            .ok_or_else(|| RoundTripFailure::new("soffice", "io", "input has no file name"))?,
    );
    Ok(fs::metadata(output).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0))
}

fn document_failure(stage: &str, document: &DumpDocument) -> RoundTripFailure {
    match &document.error {
        Some(error) => RoundTripFailure::new(stage, &error.code, &error.msg),
        None => RoundTripFailure::new(stage, "read_failed", "reader returned ok:false"),
    }
}

fn failed(stage: &str, code: impl Into<String>, msg: impl Into<String>) -> RoundTripFileMetrics {
    failed_with(RoundTripFailure::new(stage, code, msg))
}

fn failed_with(error: RoundTripFailure) -> RoundTripFileMetrics {
    RoundTripFileMetrics {
        status: "failed".to_owned(),
        error: Some(error),
        ..RoundTripFileMetrics::default()
    }
}

impl RoundTripFailure {
    fn with_stage(mut self, stage: &str) -> Self {
        self.stage = stage.to_owned();
        self
    }
}

fn capture(mut stream: impl Read) -> (Vec<u8>, bool) {
    let mut bytes = Vec::new();
    let mut limited = stream.by_ref().take(MAX_CAPTURE_BYTES + 1);
    let read_result = limited.read_to_end(&mut bytes);
    let too_large = bytes.len() as u64 > MAX_CAPTURE_BYTES;
    if too_large {
        bytes.truncate(MAX_CAPTURE_BYTES as usize);
    }
    (bytes, too_large || read_result.is_err())
}

fn join_capture(handle: thread::JoinHandle<(Vec<u8>, bool)>) -> (Vec<u8>, bool) {
    handle.join().unwrap_or_else(|_| (Vec::new(), true))
}

fn diagnostic(stderr: &[u8]) -> String {
    const MAX_DIAGNOSTIC: usize = 4096;
    let stderr = &stderr[..stderr.len().min(MAX_DIAGNOSTIC)];
    String::from_utf8_lossy(stderr).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;

    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::{
        compare_round_trip, detect_soffice, find_on_path, select_soffice_subset,
        SofficeAvailability,
    };
    use crate::model::{DumpDocument, ExpectedDump, Tool};

    fn dump(cells: Value, merges: Value) -> DumpDocument {
        let value = json!({
            "schema": 1,
            "tool": "wax",
            "toolVersion": "0.1.0",
            "file": "fixture.xlsx",
            "sha256": "abc",
            "ok": true,
            "error": null,
            "wallMs": 1,
            "peakRssBytes": null,
            "truncated": false,
            "sheets": [{
                "name": "Sheet1",
                "index": 0,
                "rows": 4,
                "cols": 2,
                "truncated": false,
                "merges": merges,
                "cells": cells
            }],
            "warnings": []
        });
        DumpDocument::parse(
            &serde_json::to_vec(&value).unwrap(),
            ExpectedDump {
                tool: Tool::Wax,
                sha256: Some("abc"),
            },
        )
        .unwrap()
    }

    fn cell(r: u64, value: Value, display: Value) -> Value {
        json!({
            "r": r, "c": 0, "t": "n", "v": value,
            "d": display, "f": null, "fmt": null
        })
    }

    #[test]
    fn comparator_reuses_numeric_epsilon_and_checks_display_and_merges() {
        let source = dump(
            json!([
                cell(0, json!(1_000_000_000.0), json!("source")),
                cell(1, json!(2), Value::Null)
            ]),
            json!(["A1:B1"]),
        );
        let target = dump(
            json!([
                cell(0, json!(1_000_000_000.5), json!("target")),
                cell(1, json!(2), Value::Null)
            ]),
            json!(["A2:B2"]),
        );

        let comparison = compare_round_trip(&source, &target);

        assert_eq!(comparison.value_match.matched, 2);
        assert_eq!(comparison.value_match.total, 2);
        assert_eq!(comparison.display_match.matched, 1);
        assert_eq!(comparison.display_match.total, 2);
        assert_eq!(comparison.merge_defects.len(), 1);
    }

    #[test]
    fn deterministic_subset_spreads_extensions_and_honors_size_cap() {
        let entries = [
            ("a.xlsx", "xlsx", 10),
            ("b.xlsx", "xlsx", 10),
            ("c.xls", "xls", 10),
            ("d.ods", "ods", 10),
            ("huge.xlsb", "xlsb", 10_000),
        ];
        let first = select_soffice_subset(entries, 3, 100, 42);
        let second = select_soffice_subset(entries, 3, 100, 42);

        assert_eq!(first, second);
        assert_eq!(first.len(), 3);
        assert!(first.contains("c.xls"));
        assert!(first.contains("d.ods"));
        assert!(!first.contains("huge.xlsb"));
    }

    #[test]
    fn soffice_detection_distinguishes_disabled_missing_and_path() {
        assert_eq!(detect_soffice(false, None), SofficeAvailability::Disabled);
        let root = tempdir().unwrap();
        let executable = root.path().join("soffice");
        fs::write(&executable, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&executable).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&executable, permissions).unwrap();
        }
        let path = OsString::from(root.path());
        assert_eq!(find_on_path(&path, "soffice"), Some(executable.clone()));
        assert_eq!(
            detect_soffice(true, Some(&executable)),
            SofficeAvailability::Available { executable }
        );
        assert!(matches!(
            detect_soffice(true, Some(&root.path().join("missing"))),
            SofficeAvailability::Unavailable { .. }
        ));
    }
}

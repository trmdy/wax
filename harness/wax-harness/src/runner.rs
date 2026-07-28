use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use wait_timeout::ChildExt;

use crate::aggregate::{aggregate_with_serve, Scoreboard};
use crate::compare::{compare, FileMetrics, ToolObservation};
use crate::formats::{aggregate_format_coverage, load_corpus_format_index};
use crate::model::{DumpDocument, ExpectedDump, Tool};
use crate::render::render_markdown_with_formats;
use crate::serve::{
    detect_serve, run_serve_file, ServeAvailability, ServeFileConfig, ServeFileMetrics,
};
use crate::triage::render_triage;

const DEFAULT_MAX_CELLS: u64 = 200_000;
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_CAPTURE_BYTES: u64 = 256 * 1024 * 1024;
const EXPORT_SMOKE_FILES: usize = 50;

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub repo_root: PathBuf,
    pub manifest: PathBuf,
    pub limit: Option<usize>,
    pub jobs: usize,
    pub max_cells: u64,
    pub timeout_ms: u64,
    pub serve_enabled: bool,
    pub wax_bin: PathBuf,
    pub node_bin: PathBuf,
    pub oracle_script: PathBuf,
}

#[derive(Debug)]
pub struct RunnerReport {
    pub scoreboard: Scoreboard,
    pub results_path: PathBuf,
    pub scoreboard_json_path: PathBuf,
    pub scoreboard_markdown_path: PathBuf,
    pub format_coverage_path: PathBuf,
    pub triage_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ManifestEntry {
    id: String,
    path: String,
    sha256: String,
    bytes: u64,
    ext: String,
    collection: String,
    source: String,
    licence: String,
    fetched_at: String,
    private: bool,
}

#[derive(Debug)]
struct CapturedProcess {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    output_too_large: bool,
    elapsed_ms: f64,
}

pub fn run(config: RunnerConfig) -> Result<RunnerReport> {
    validate_config(&config)?;
    let mut entries = read_manifest(&config.manifest)?;
    if let Some(limit) = config.limit {
        entries.truncate(limit);
    }

    let before_skip = entries.len();
    entries.retain(|entry| !should_skip_private(entry, &config.repo_root));
    let skipped = (before_skip - entries.len()) as u64;
    let serve_availability = if config.serve_enabled {
        detect_serve(
            &config.wax_bin,
            &config.repo_root,
            Duration::from_millis(config.timeout_ms),
        )
    } else {
        ServeAvailability::Disabled
    };
    let serve_available = serve_availability.is_available();
    let export_smoke_ids: HashSet<_> = entries
        .iter()
        .filter(|entry| entry.ext.eq_ignore_ascii_case("xlsx"))
        .take(EXPORT_SMOKE_FILES)
        .map(|entry| entry.id.clone())
        .collect();

    let harness_dir = config.repo_root.join("harness");
    fs::create_dir_all(&harness_dir)
        .with_context(|| format!("failed to create {}", harness_dir.display()))?;
    let results_path = harness_dir.join("results.jsonl");
    let scoreboard_json_path = harness_dir.join("scoreboard.json");
    let format_coverage_path = harness_dir.join("format-coverage.json");
    let triage_path = harness_dir.join("triage.md");
    let scoreboard_markdown_path = config.repo_root.join("SCOREBOARD.md");

    let results_file = File::create(&results_path)
        .with_context(|| format!("failed to create {}", results_path.display()))?;
    let mut results_writer = BufWriter::new(results_file);
    let config = Arc::new(config);
    let entries = Arc::new(entries);
    let next = Arc::new(AtomicUsize::new(0));
    let worker_count = config.jobs.min(entries.len().max(1));
    let (sender, receiver) = mpsc::sync_channel(worker_count * 2);

    let mut results = thread::scope(|scope| -> Result<Vec<FileMetrics>> {
        for _ in 0..worker_count {
            let config = Arc::clone(&config);
            let entries = Arc::clone(&entries);
            let next = Arc::clone(&next);
            let sender = sender.clone();
            let export_smoke_ids = &export_smoke_ids;
            scope.spawn(move || loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(entry) = entries.get(index) else {
                    break;
                };
                let export_smoke = export_smoke_ids.contains(&entry.id);
                let result = std::panic::catch_unwind(|| {
                    process_entry(entry, &config, serve_available, export_smoke)
                })
                .unwrap_or_else(|_| internal_failure(entry));
                if sender.send(result).is_err() {
                    break;
                }
            });
        }
        drop(sender);

        let mut results = Vec::with_capacity(entries.len());
        for result in receiver {
            serde_json::to_writer(&mut results_writer, &result)
                .context("failed to serialize a per-file result")?;
            results_writer
                .write_all(b"\n")
                .context("failed to stream a per-file result")?;
            results.push(result);
        }
        results_writer
            .flush()
            .context("failed to flush results.jsonl")?;
        Ok(results)
    })?;
    results.sort_by(|left, right| left.id.cmp(&right.id));

    let generated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let scoreboard =
        aggregate_with_serve(&results, skipped, generated_at.clone(), &serve_availability);
    let corpus_formats =
        load_corpus_format_index(&config.repo_root.join("harness/formats/corpus-formats.json"))?;
    let format_coverage =
        aggregate_format_coverage(&results, generated_at.clone(), corpus_formats.as_ref());
    let triage = render_triage(&results, &generated_at);
    write_json_atomic(&scoreboard_json_path, &scoreboard)?;
    write_json_atomic(&format_coverage_path, &format_coverage)?;
    write_bytes_atomic(&triage_path, triage.as_bytes())?;
    write_bytes_atomic(
        &scoreboard_markdown_path,
        render_markdown_with_formats(&scoreboard, Some(&format_coverage)).as_bytes(),
    )?;

    Ok(RunnerReport {
        scoreboard,
        results_path,
        scoreboard_json_path,
        scoreboard_markdown_path,
        format_coverage_path,
        triage_path,
    })
}

impl RunnerConfig {
    pub fn from_environment(
        repo_root: PathBuf,
        manifest: PathBuf,
        limit: Option<usize>,
        jobs: Option<usize>,
        max_cells: Option<u64>,
        timeout_ms: Option<u64>,
        serve_enabled: bool,
    ) -> Self {
        let repo_root = if repo_root.is_absolute() {
            repo_root
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(repo_root)
        };
        let relative_to_root = |path: PathBuf| {
            if path.is_absolute() {
                path
            } else {
                repo_root.join(path)
            }
        };
        let wax_bin = std::env::var_os("WAX_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/release/wax"));
        let node_bin = std::env::var_os("WAX_NODE_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("node"));
        let oracle_script = std::env::var_os("WAX_ORACLE_SCRIPT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("harness/oracle/run.js"));

        Self {
            manifest: relative_to_root(manifest),
            jobs: jobs.unwrap_or_else(default_jobs),
            max_cells: max_cells.unwrap_or(DEFAULT_MAX_CELLS),
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
            serve_enabled,
            wax_bin: relative_to_root(wax_bin),
            node_bin,
            oracle_script: relative_to_root(oracle_script),
            repo_root,
            limit,
        }
    }
}

fn default_jobs() -> usize {
    thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
}

fn validate_config(config: &RunnerConfig) -> Result<()> {
    if config.jobs == 0 {
        bail!("--jobs must be greater than zero");
    }
    if config.max_cells == 0 {
        bail!("--max-cells must be greater than zero");
    }
    if config.timeout_ms == 0 {
        bail!("--timeout-ms must be greater than zero");
    }
    if !config.repo_root.is_dir() {
        bail!("repo root does not exist: {}", config.repo_root.display());
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<Vec<ManifestEntry>> {
    let file =
        File::open(path).with_context(|| format!("failed to open manifest {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line.with_context(|| format!("failed to read manifest line {line_number}"))?;
        if line.trim().is_empty() {
            bail!("manifest line {line_number} is empty");
        }
        let entry: ManifestEntry = serde_json::from_str(&line)
            .with_context(|| format!("invalid manifest line {line_number}"))?;
        validate_manifest_entry(&entry, line_number)?;
        if let Some(previous) = entries.last() {
            let previous: &ManifestEntry = previous;
            if previous.id >= entry.id {
                bail!(
                    "manifest ids must be unique and sorted: line {line_number} has {:?} after {:?}",
                    entry.id,
                    previous.id
                );
            }
        }
        entries.push(entry);
    }
    Ok(entries)
}

fn validate_manifest_entry(entry: &ManifestEntry, line_number: usize) -> Result<()> {
    if entry.id.trim().is_empty()
        || entry.path.trim().is_empty()
        || entry.sha256.trim().is_empty()
        || entry.ext.trim().is_empty()
        || entry.collection.trim().is_empty()
        || entry.source.trim().is_empty()
        || entry.licence.trim().is_empty()
        || entry.fetched_at.trim().is_empty()
    {
        bail!("manifest line {line_number} contains an empty required string");
    }
    let _ = entry.bytes;
    Ok(())
}

fn should_skip_private(entry: &ManifestEntry, repo_root: &Path) -> bool {
    if !entry.private {
        return false;
    }
    let path = Path::new(&entry.path);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    };
    !resolved.exists()
}

fn process_entry(
    entry: &ManifestEntry,
    config: &RunnerConfig,
    serve_available: bool,
    export_smoke: bool,
) -> FileMetrics {
    let wax = invoke(
        Tool::Wax,
        &config.wax_bin,
        &[
            "dump".to_owned(),
            "--json".to_owned(),
            entry.path.clone(),
            "--max-cells".to_owned(),
            config.max_cells.to_string(),
            "--timeout-ms".to_owned(),
            config.timeout_ms.to_string(),
        ],
        entry,
        config,
    );
    let oracle_script = config.oracle_script.to_string_lossy().into_owned();
    let sheetjs = invoke(
        Tool::Sheetjs,
        &config.node_bin,
        &[
            oracle_script,
            entry.path.clone(),
            "--max-cells".to_owned(),
            config.max_cells.to_string(),
            "--timeout-ms".to_owned(),
            config.timeout_ms.to_string(),
        ],
        entry,
        config,
    );

    let mut result = compare(
        entry.id.clone(),
        entry.path.clone(),
        entry.sha256.clone(),
        &wax,
        &sheetjs,
    );
    apply_manifest_metadata(&mut result, entry);
    if serve_available {
        result.serve = Some(run_serve_entry(entry, config, export_smoke));
    }
    result
}

fn run_serve_entry(
    entry: &ManifestEntry,
    config: &RunnerConfig,
    export_smoke: bool,
) -> ServeFileMetrics {
    let path = Path::new(&entry.path);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        config.repo_root.join(path)
    };
    run_serve_file(ServeFileConfig {
        wax_bin: &config.wax_bin,
        repo_root: &config.repo_root,
        file: &path,
        timeout: Duration::from_millis(config.timeout_ms),
        export_smoke,
    })
}

fn internal_failure(entry: &ManifestEntry) -> FileMetrics {
    let wax = ToolObservation::failure(
        Tool::Wax,
        "internal_error",
        "wax harness worker panicked",
        None,
    );
    let sheetjs = ToolObservation::failure(
        Tool::Sheetjs,
        "internal_error",
        "wax harness worker panicked",
        None,
    );
    let mut result = compare(
        entry.id.clone(),
        entry.path.clone(),
        entry.sha256.clone(),
        &wax,
        &sheetjs,
    );
    apply_manifest_metadata(&mut result, entry);
    result
}

fn apply_manifest_metadata(result: &mut FileMetrics, entry: &ManifestEntry) {
    result.ext = entry.ext.to_ascii_lowercase();
    result.private = entry.private;
}

fn invoke(
    tool: Tool,
    executable: &Path,
    arguments: &[String],
    entry: &ManifestEntry,
    config: &RunnerConfig,
) -> ToolObservation {
    let started = Instant::now();
    let child = Command::new(executable)
        .args(arguments)
        .current_dir(&config.repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            return ToolObservation::failure(
                tool,
                "spawn_failed",
                format!("failed to spawn {}: {error}", executable.display()),
                Some(elapsed_ms(started)),
            );
        }
    };

    let stdout = child.stdout.take().expect("piped stdout must be present");
    let stderr = child.stderr.take().expect("piped stderr must be present");
    let stdout_reader = thread::spawn(move || capture(stdout));
    let stderr_reader = thread::spawn(move || capture(stderr));
    let timeout = Duration::from_millis(config.timeout_ms);
    let wait_result = child.wait_timeout(timeout);

    let status = match wait_result {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return ToolObservation::failure(
                tool,
                "timeout",
                format!("process exceeded {} ms", config.timeout_ms),
                Some(elapsed_ms(started)),
            );
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return ToolObservation::failure(
                tool,
                "wait_failed",
                format!("failed while waiting for process: {error}"),
                Some(elapsed_ms(started)),
            );
        }
    };

    let (stdout, stdout_too_large) = join_capture(stdout_reader);
    let (stderr, stderr_too_large) = join_capture(stderr_reader);
    let captured = CapturedProcess {
        status,
        stdout,
        stderr,
        output_too_large: stdout_too_large || stderr_too_large,
        elapsed_ms: elapsed_ms(started),
    };
    observation_from_capture(tool, captured, entry)
}

fn observation_from_capture(
    tool: Tool,
    captured: CapturedProcess,
    entry: &ManifestEntry,
) -> ToolObservation {
    if captured.output_too_large {
        return ToolObservation::failure(
            tool,
            "output_too_large",
            format!("process output exceeded {MAX_CAPTURE_BYTES} bytes"),
            Some(captured.elapsed_ms),
        );
    }
    if !captured.status.success() {
        let code = crash_code(&captured.status);
        let stderr = diagnostic(&captured.stderr);
        let message = if stderr.is_empty() {
            format!("process exited with {}", captured.status)
        } else {
            format!("process exited with {}: {stderr}", captured.status)
        };
        return ToolObservation::failure(tool, code, message, Some(captured.elapsed_ms));
    }

    match DumpDocument::parse(
        &captured.stdout,
        ExpectedDump {
            tool,
            sha256: Some(&entry.sha256),
        },
    ) {
        Ok(document) => ToolObservation::document(document),
        Err(error) => ToolObservation::failure(
            tool,
            "schema_violation",
            error.to_string(),
            Some(captured.elapsed_ms),
        ),
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

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn crash_code(status: &ExitStatus) -> &'static str {
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

fn write_json_atomic(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).context("failed to render JSON report")?;
    bytes.push(b'\n');
    write_bytes_atomic(path, &bytes)
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("output path has no UTF-8 file name")?;
    let temporary = path.with_file_name(format!(".{file_name}.tmp-{}", std::process::id()));
    fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{should_skip_private, ManifestEntry};
    use tempfile::tempdir;

    fn private_entry(path: String) -> ManifestEntry {
        ManifestEntry {
            id: "private/file.xlsx".to_owned(),
            path,
            sha256: "abc".to_owned(),
            bytes: 1,
            ext: "xlsx".to_owned(),
            collection: "private".to_owned(),
            source: "local".to_owned(),
            licence: "private".to_owned(),
            fetched_at: "2026-07-28T00:00:00Z".to_owned(),
            private: true,
        }
    }

    #[test]
    fn missing_private_files_are_skipped_but_existing_ones_are_not() {
        let root = tempdir().unwrap();
        let missing = private_entry(root.path().join("missing.xlsx").display().to_string());
        let present_path = root.path().join("present.xlsx");
        std::fs::write(&present_path, b"fixture").unwrap();
        let present = private_entry(present_path.display().to_string());

        assert!(should_skip_private(&missing, root.path()));
        assert!(!should_skip_private(&present, root.path()));
    }
}

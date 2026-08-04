use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use serde_json::json;
use sha2::{Digest, Sha256};
use wax_eval::{FormulaWorkbook, DEFAULT_EVAL_BUDGET};
use wax_proto::PROTO_VERSION;
use wax_read::{read_with_deadline, CalamineReader, ReaderOptions};
use wax_store::WorkbookStore;

mod serve;

const USAGE: &str = "Usage:
  wax --version
  wax dump --json <file> [--max-cells N] [--max-bytes N] [--timeout-ms N]
  wax export --json <in> <out> --format xlsx|csv [--sheet N] [--overrides <json-file>] [--max-cells N] [--max-bytes N] [--timeout-ms N]
  wax serve [--idle-timeout-ms N] [--max-handles N]";

fn main() -> ExitCode {
    ExitCode::from(run() as u8)
}

fn run() -> i32 {
    let arguments: Vec<OsString> = env::args_os().skip(1).collect();
    if arguments.as_slice() == [OsStr::new("--version")] {
        println!(
            "wax {} (proto {})",
            env!("CARGO_PKG_VERSION"),
            PROTO_VERSION
        );
        return 0;
    }
    if arguments.as_slice() == [OsStr::new("--help")] || arguments.as_slice() == [OsStr::new("-h")]
    {
        println!("{USAGE}");
        return 0;
    }

    let command = match parse_arguments(&arguments) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            return 2;
        }
    };

    match command {
        Command::Dump(command) => run_dump(command),
        Command::Export(command) => run_export(command),
        Command::Serve(config) => match serve::run(config) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("wax serve: {error}");
                1
            }
        },
    }
}

fn run_dump(command: DumpCommand) -> i32 {
    let started = Instant::now();
    let mut document = read_with_deadline(
        CalamineReader,
        &command.path,
        ReaderOptions {
            max_cells: command.max_cells,
            max_bytes: command.max_bytes,
            timeout_ms: command.timeout_ms,
            ..ReaderOptions::default()
        },
    );
    let skip_hash = document
        .error
        .as_ref()
        .is_some_and(|error| matches!(error.code.as_str(), "timeout" | "too_large"));
    let sha256 = if skip_hash {
        String::new()
    } else {
        match sha256_file(&command.path) {
            Ok(sha256) => sha256,
            Err(error) => {
                eprintln!("wax: could not hash {}: {error}", command.path.display());
                return 1;
            }
        }
    };
    document.wall_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    document.peak_rss_bytes = peak_rss_bytes();
    document.sha256 = sha256;
    document.file = display_path(&command.path);
    document.tool_version = env!("CARGO_PKG_VERSION").to_owned();

    let stdout = io::stdout();
    let mut output = stdout.lock();
    if let Err(error) = serde_json::to_writer(&mut output, &document)
        .and_then(|()| output.write_all(b"\n").map_err(serde_json::Error::io))
    {
        eprintln!("wax: could not write dump JSON: {error}");
        return 1;
    }
    0
}

fn run_export(command: ExportCommand) -> i32 {
    let document = read_with_deadline(
        CalamineReader,
        &command.input,
        ReaderOptions {
            max_cells: command.max_cells,
            max_bytes: command.max_bytes,
            timeout_ms: command.timeout_ms,
            ..ReaderOptions::default()
        },
    );
    if !document.ok {
        let error = document.error.unwrap_or_else(|| wax_core::DumpError {
            code: "internal".to_owned(),
            msg: "reader returned a failure without error details".to_owned(),
        });
        return write_export_json(json!({
            "ok": false,
            "code": error.code,
            "msg": error.msg,
        }));
    }

    let overrides = match &command.overrides {
        None => Vec::new(),
        Some(path) => match load_overrides(path) {
            Ok(overrides) => overrides,
            Err((code, msg)) => {
                return write_export_json(json!({
                    "ok": false,
                    "code": code,
                    "msg": msg,
                }))
            }
        },
    };

    let mut warnings = document.warnings.clone();
    let store = WorkbookStore::from_document(document);
    if store.sheet_meta(command.sheet).is_none() {
        return write_export_json(json!({
            "ok": false,
            "code": "bad_request",
            "msg": format!("sheet index {} is out of range", command.sheet),
        }));
    }

    let formula_supported = command
        .input
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_ascii_lowercase().as_str(), "xlsx" | "xlsm")
        });
    let (formula, opened_evaluation) = if formula_supported {
        FormulaWorkbook::open(&store, DEFAULT_EVAL_BUDGET)
    } else {
        FormulaWorkbook::file_cached(&store)
    };
    warnings.extend(opened_evaluation.warnings);
    // v0.2 A6: CSV ignores edits aimed at other sheets. Its formula layer
    // must use the same scope or a cross-sheet formula could observe an edit
    // the CSV writer intentionally does not apply.
    let evaluation_overrides = overrides
        .iter()
        .filter(|entry| command.format == ExportFormat::Xlsx || entry.sheet == command.sheet)
        .cloned()
        .collect::<Vec<_>>();
    let recalculated = match formula.recalculate(&store, &evaluation_overrides, DEFAULT_EVAL_BUDGET)
    {
        Ok(outcome) => outcome,
        Err(error) => {
            return write_export_json(json!({
                "ok": false,
                "code": error.code,
                "msg": error.msg,
            }))
        }
    };
    warnings.extend(recalculated.warnings.clone());

    let cancel = AtomicBool::new(false);
    let result = match command.format {
        ExportFormat::Xlsx => wax_write::write_xlsx_with_evaluated_overrides(
            &store,
            &command.output,
            &overrides,
            &wax_core::SizeOverrides::default(),
            &recalculated.all_evaluated,
            &cancel,
        ),
        ExportFormat::Csv => wax_write::write_csv_with_evaluated_overrides(
            &store,
            command.sheet,
            &command.output,
            &overrides,
            &wax_core::SizeOverrides::default(),
            &recalculated.all_evaluated,
            &cancel,
        ),
    };
    match result {
        Ok(mut outcome) => {
            let unevaluated = if command.format == ExportFormat::Csv {
                formula.unevaluated_formulas_on_sheet(command.sheet)
            } else {
                formula.unevaluated_formulas()
            };
            if unevaluated > 0 {
                outcome.dropped.push(format!(
                    "formulas kept file-cached values ({unevaluated} unevaluated)"
                ));
            }
            outcome.dropped.extend(warnings);
            write_export_json(json!({
                "ok": true,
                "bytes": outcome.bytes,
                "applied": outcome.applied,
                "dropped": outcome.dropped,
            }))
        }
        Err(error) => write_export_json(json!({
            "ok": false,
            "code": error.code,
            "msg": error.msg,
        })),
    }
}

/// Load an overrides JSON file (literal `{sheet, r, c, v}` or authored
/// formula `{sheet, r, c, f, v?}` entries, the same shape the serve
/// `export` op accepts).
fn load_overrides(path: &Path) -> Result<Vec<wax_core::CellOverride>, (String, String)> {
    let raw = std::fs::read_to_string(path).map_err(|error| {
        (
            "bad_request".to_owned(),
            format!("could not read overrides file {}: {error}", path.display()),
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
        (
            "bad_request".to_owned(),
            format!(
                "overrides file {} is not valid JSON: {error}",
                path.display()
            ),
        )
    })?;
    wax_proto::parse_overrides(&value).map_err(|msg| ("bad_request".to_owned(), msg))
}

fn write_export_json(value: serde_json::Value) -> i32 {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if let Err(error) = serde_json::to_writer(&mut output, &value)
        .and_then(|()| output.write_all(b"\n").map_err(serde_json::Error::io))
    {
        eprintln!("wax: could not write export JSON: {error}");
        return 1;
    }
    0
}

#[derive(Debug)]
enum Command {
    Dump(DumpCommand),
    Export(ExportCommand),
    Serve(serve::Config),
}

#[derive(Debug)]
struct DumpCommand {
    path: PathBuf,
    max_cells: usize,
    max_bytes: u64,
    timeout_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExportFormat {
    Xlsx,
    Csv,
}

#[derive(Debug)]
struct ExportCommand {
    input: PathBuf,
    output: PathBuf,
    format: ExportFormat,
    sheet: u32,
    overrides: Option<PathBuf>,
    max_cells: usize,
    max_bytes: u64,
    timeout_ms: u64,
}

fn parse_arguments(arguments: &[OsString]) -> Result<Command, String> {
    match arguments.first().and_then(|argument| argument.to_str()) {
        Some("dump") => parse_dump_arguments(arguments).map(Command::Dump),
        Some("export") => parse_export_arguments(arguments).map(Command::Export),
        Some("serve") => parse_serve_arguments(arguments).map(Command::Serve),
        Some(command) => Err(format!("wax: unknown command `{command}`")),
        None => Err("wax: expected a command".to_owned()),
    }
}

fn parse_dump_arguments(arguments: &[OsString]) -> Result<DumpCommand, String> {
    let mut saw_json = false;
    let mut path = None;
    let mut max_cells = ReaderOptions::default().max_cells;
    let mut max_bytes = ReaderOptions::default().max_bytes;
    let mut timeout_ms = ReaderOptions::default().timeout_ms;
    let mut index = 1;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--json") => saw_json = true,
            Some("--max-cells") => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "wax: --max-cells requires a value".to_owned())?;
                max_cells = parse_number(value, "--max-cells")?;
            }
            Some("--timeout-ms") => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "wax: --timeout-ms requires a value".to_owned())?;
                timeout_ms = parse_number(value, "--timeout-ms")?;
            }
            Some("--max-bytes") => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "wax: --max-bytes requires a value".to_owned())?;
                max_bytes = parse_number(value, "--max-bytes")?;
            }
            Some(flag) if flag.starts_with('-') => {
                return Err(format!("wax: unknown option `{flag}`"));
            }
            _ if path.is_none() => path = Some(PathBuf::from(&arguments[index])),
            _ => return Err("wax: dump accepts exactly one input file".to_owned()),
        }
        index += 1;
    }

    if !saw_json {
        return Err("wax: dump requires --json".to_owned());
    }
    let path = path.ok_or_else(|| "wax: dump requires an input file".to_owned())?;
    Ok(DumpCommand {
        path,
        max_cells,
        max_bytes,
        timeout_ms,
    })
}

fn parse_export_arguments(arguments: &[OsString]) -> Result<ExportCommand, String> {
    let mut saw_json = false;
    let mut paths = Vec::with_capacity(2);
    let mut format = None;
    let mut sheet = 0;
    let mut overrides = None;
    let mut max_cells = ReaderOptions::default().max_cells;
    let mut max_bytes = ReaderOptions::default().max_bytes;
    let mut timeout_ms = ReaderOptions::default().timeout_ms;
    let mut index = 1;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--json") => saw_json = true,
            Some("--format") => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "wax: --format requires a value".to_owned())?;
                format = Some(match value.to_str() {
                    Some("xlsx") => ExportFormat::Xlsx,
                    Some("csv") => ExportFormat::Csv,
                    Some(value) => {
                        return Err(format!(
                            "wax: --format must be `xlsx` or `csv`, got `{value}`"
                        ))
                    }
                    None => return Err("wax: --format must be valid UTF-8".to_owned()),
                });
            }
            Some("--sheet") => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "wax: --sheet requires a value".to_owned())?;
                sheet = parse_number(value, "--sheet")?;
            }
            Some("--overrides") => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "wax: --overrides requires a file path".to_owned())?;
                overrides = Some(PathBuf::from(value));
            }
            Some("--max-cells") => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "wax: --max-cells requires a value".to_owned())?;
                max_cells = parse_number(value, "--max-cells")?;
            }
            Some("--timeout-ms") => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "wax: --timeout-ms requires a value".to_owned())?;
                timeout_ms = parse_number(value, "--timeout-ms")?;
            }
            Some("--max-bytes") => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "wax: --max-bytes requires a value".to_owned())?;
                max_bytes = parse_number(value, "--max-bytes")?;
            }
            Some(flag) if flag.starts_with('-') => {
                return Err(format!("wax: unknown option `{flag}`"));
            }
            _ if paths.len() < 2 => paths.push(PathBuf::from(&arguments[index])),
            _ => return Err("wax: export accepts exactly one input and one output file".to_owned()),
        }
        index += 1;
    }

    if !saw_json {
        return Err("wax: export requires --json".to_owned());
    }
    if paths.len() != 2 {
        return Err("wax: export requires an input and output file".to_owned());
    }
    let format = format.ok_or_else(|| "wax: export requires --format xlsx|csv".to_owned())?;
    Ok(ExportCommand {
        input: paths.remove(0),
        output: paths.remove(0),
        format,
        sheet,
        overrides,
        max_cells,
        max_bytes,
        timeout_ms,
    })
}

fn parse_serve_arguments(arguments: &[OsString]) -> Result<serve::Config, String> {
    let mut config = serve::Config::default();
    let mut index = 1;
    while index < arguments.len() {
        let flag = arguments[index]
            .to_str()
            .ok_or_else(|| "wax: serve options must be valid UTF-8".to_owned())?;
        match flag {
            "--idle-timeout-ms" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "wax: --idle-timeout-ms requires a value".to_owned())?;
                config.idle_timeout_ms = parse_number(value, "--idle-timeout-ms")?;
            }
            "--max-handles" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "wax: --max-handles requires a value".to_owned())?;
                config.max_handles = parse_number(value, "--max-handles")?;
            }
            _ => return Err(format!("wax: unknown option `{flag}`")),
        }
        index += 1;
    }
    Ok(config)
}

fn parse_number<T>(value: &OsStr, flag: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    value
        .to_str()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("wax: {flag} requires a non-negative integer"))
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut input = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn display_path(path: &Path) -> String {
    let canonical = path.canonicalize().ok();
    let current = env::current_dir().ok();
    if let (Some(canonical), Some(current)) = (&canonical, current) {
        if let Some(root) = current
            .ancestors()
            .find(|ancestor| ancestor.join(".git").exists())
        {
            if let Ok(relative) = canonical.strip_prefix(root) {
                return relative.to_string_lossy().into_owned();
            }
        }
    }
    canonical
        .as_deref()
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes the provided rusage value when it returns zero.
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    // SAFETY: the successful getrusage call above initialized usage.
    let usage = unsafe { usage.assume_init() };
    let rss = u64::try_from(usage.ru_maxrss).ok()?;
    #[cfg(target_os = "macos")]
    {
        Some(rss)
    }
    #[cfg(target_os = "linux")]
    {
        rss.checked_mul(1024)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn peak_rss_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn serve_defaults_match_contract() {
        let Command::Serve(config) = parse_arguments(&args(&["serve"])).expect("should parse")
        else {
            panic!("expected serve command");
        };
        assert_eq!(config.idle_timeout_ms, 300_000);
        assert_eq!(config.max_handles, 16);
    }

    #[test]
    fn serve_options_parse_in_either_order() {
        let Command::Serve(config) = parse_arguments(&args(&[
            "serve",
            "--max-handles",
            "3",
            "--idle-timeout-ms",
            "20",
        ]))
        .expect("should parse") else {
            panic!("expected serve command");
        };
        assert_eq!(config.idle_timeout_ms, 20);
        assert_eq!(config.max_handles, 3);
    }

    #[test]
    fn serve_rejects_positional_and_invalid_options() {
        for values in [
            vec!["serve", "file.xlsx"],
            vec!["serve", "--max-handles"],
            vec!["serve", "--idle-timeout-ms", "-1"],
        ] {
            assert!(parse_arguments(&args(&values)).is_err());
        }
    }

    #[test]
    fn export_options_and_paths_parse_in_any_order() {
        let Command::Export(command) = parse_arguments(&args(&[
            "export",
            "--timeout-ms",
            "20",
            "in.xlsx",
            "--format",
            "csv",
            "--json",
            "--sheet",
            "2",
            "out.csv",
            "--max-cells",
            "3",
            "--max-bytes",
            "40",
        ]))
        .expect("should parse") else {
            panic!("expected export command");
        };
        assert_eq!(command.input, PathBuf::from("in.xlsx"));
        assert_eq!(command.output, PathBuf::from("out.csv"));
        assert_eq!(command.format, ExportFormat::Csv);
        assert_eq!(command.sheet, 2);
        assert_eq!(command.max_cells, 3);
        assert_eq!(command.max_bytes, 40);
        assert_eq!(command.timeout_ms, 20);
    }

    #[test]
    fn export_rejects_invalid_format_sheet_and_missing_arguments() {
        for values in [
            vec!["export", "--json", "in.xlsx", "out.xlsx", "--format", "pdf"],
            vec![
                "export", "--json", "in.xlsx", "out.xlsx", "--format", "xlsx", "--sheet", "-1",
            ],
            vec!["export", "--json", "in.xlsx", "--format", "xlsx"],
            vec!["export", "in.xlsx", "out.xlsx", "--format", "xlsx"],
            vec!["export", "--json", "in.xlsx", "out.xlsx"],
        ] {
            assert!(
                parse_arguments(&args(&values)).is_err(),
                "unexpectedly accepted {values:?}"
            );
        }
    }
}

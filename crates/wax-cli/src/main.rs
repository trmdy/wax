use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use sha2::{Digest, Sha256};
use wax_proto::PROTO_VERSION;
use wax_read::{CalamineReader, Reader, ReaderOptions};

const USAGE: &str = "Usage:
  wax --version
  wax dump --json <file> [--max-cells N] [--timeout-ms N]";

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

    let command = match parse_dump_arguments(&arguments) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            return 2;
        }
    };

    let sha256 = match sha256_file(&command.path) {
        Ok(sha256) => sha256,
        Err(error) => {
            eprintln!("wax: could not hash {}: {error}", command.path.display());
            return 1;
        }
    };

    let started = Instant::now();
    let mut document = CalamineReader.read(
        &command.path,
        ReaderOptions {
            max_cells: command.max_cells,
            timeout_ms: command.timeout_ms,
        },
    );
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

#[derive(Debug)]
struct DumpCommand {
    path: PathBuf,
    max_cells: usize,
    timeout_ms: u64,
}

fn parse_dump_arguments(arguments: &[OsString]) -> Result<DumpCommand, String> {
    if arguments.first().is_none_or(|argument| argument != "dump") {
        return Err("wax: expected the `dump` command".to_owned());
    }

    let mut saw_json = false;
    let mut path = None;
    let mut max_cells = ReaderOptions::default().max_cells;
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
        timeout_ms,
    })
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
fn peak_rss_bytes() -> Option<u64> {
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
fn peak_rss_bytes() -> Option<u64> {
    None
}

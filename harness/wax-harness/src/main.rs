use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use wax_harness::{run, RunnerConfig};

#[derive(Debug, Parser)]
#[command(
    name = "wax-harness",
    version,
    about = "Run the wax differential corpus harness"
)]
struct Arguments {
    #[arg(long, default_value = "corpus/manifest.jsonl")]
    manifest: PathBuf,

    #[arg(long)]
    limit: Option<usize>,

    #[arg(long)]
    jobs: Option<usize>,

    #[arg(long)]
    max_cells: Option<u64>,

    #[arg(long)]
    timeout_ms: Option<u64>,

    /// Skip the wax serve protocol pass.
    #[arg(long)]
    no_serve: bool,

    #[arg(long, default_value = ".", hide = true)]
    repo_root: PathBuf,
}

fn main() -> Result<()> {
    let arguments = Arguments::parse();
    let config = RunnerConfig::from_environment(
        arguments.repo_root,
        arguments.manifest,
        arguments.limit,
        arguments.jobs,
        arguments.max_cells,
        arguments.timeout_ms,
        !arguments.no_serve,
    );
    let report = run(config)?;
    eprintln!(
        "wax harness: attempted {}, skipped {}; wrote {}, {}, {}, {}, and {}",
        report.scoreboard.files_attempted,
        report.scoreboard.files_skipped,
        report.results_path.display(),
        report.scoreboard_json_path.display(),
        report.scoreboard_markdown_path.display(),
        report.format_coverage_path.display(),
        report.triage_path.display()
    );
    Ok(())
}

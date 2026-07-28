pub mod aggregate;
pub mod compare;
pub mod formats;
pub mod model;
pub mod render;
pub mod runner;
pub mod triage;

pub use aggregate::{aggregate, Scoreboard};
pub use compare::{compare, FileMetrics, ToolObservation};
pub use formats::{
    aggregate_format_coverage, load_corpus_format_index, CorpusFormatIndex, FormatCoverageReport,
};
pub use model::{DumpDocument, DumpError, ExpectedDump, SchemaError, Tool};
pub use render::{render_markdown, render_markdown_with_formats};
pub use runner::{run, RunnerConfig, RunnerReport};
pub use triage::render_triage;

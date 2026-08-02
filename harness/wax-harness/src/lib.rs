pub mod aggregate;
pub mod compare;
pub mod formats;
pub mod model;
pub mod render;
pub mod roundtrip;
pub mod runner;
pub mod serve;
pub mod triage;

pub use aggregate::{
    aggregate, aggregate_with_serve, aggregate_with_serve_and_round_trip, RoundTripMetrics,
    RoundTripStatusMetric, Scoreboard,
};
pub use compare::{compare, FileMetrics, ToolObservation};
pub use formats::{
    aggregate_format_coverage, load_corpus_format_index, CorpusFormatIndex, FormatCoverageReport,
};
pub use model::{DumpDocument, DumpError, ExpectedDump, SchemaError, Tool};
pub use render::{render_markdown, render_markdown_with_formats};
pub use roundtrip::{
    compare_round_trip, detect_soffice, select_soffice_subset, RoundTripComparison,
    RoundTripFileMetrics, SofficeAvailability,
};
pub use runner::{run, RunnerConfig, RunnerPasses, RunnerReport};
pub use serve::{
    detect_serve, run_serve_file, window_offsets, ExportSmokeMetric, FormulaDisagreement,
    FormulaEvalMetric, FormulaProbe, ServeAvailability, ServeFailure, ServeFileConfig,
    ServeFileMetrics, ServeRequestMetric,
};
pub use triage::render_triage;

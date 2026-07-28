pub mod aggregate;
pub mod compare;
pub mod model;
pub mod render;
pub mod runner;

pub use aggregate::{aggregate, Scoreboard};
pub use compare::{compare, FileMetrics, ToolObservation};
pub use model::{DumpDocument, DumpError, ExpectedDump, SchemaError, Tool};
pub use render::render_markdown;
pub use runner::{run, RunnerConfig, RunnerReport};

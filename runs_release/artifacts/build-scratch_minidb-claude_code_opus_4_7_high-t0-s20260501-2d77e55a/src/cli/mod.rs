//! Command-line entry point: argument parsing, REPL, batch evaluator,
//! and result formatting (table, CSV, JSON).

mod args;
mod format;
mod repl;

pub use args::{parse_args, CliArgs, OutputFormat, RunMode};
pub use format::Formatter;
pub use repl::{run, run_with_io};

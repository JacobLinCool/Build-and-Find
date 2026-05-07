//! Argument parsing for the `minidb` binary.

use std::path::PathBuf;

use crate::error::{MiniDbError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "table" => Ok(Self::Table),
            "csv" => Ok(Self::Csv),
            "json" => Ok(Self::Json),
            other => Err(MiniDbError::cli(format!(
                "unknown --format value '{}'; expected table|csv|json",
                other
            ))),
        }
    }
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Table
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunMode {
    Repl,
    Eval(String),
    File(PathBuf),
}

#[derive(Clone, Debug)]
pub struct CliArgs {
    pub db: PathBuf,
    pub mode: Option<RunMode>,
    pub format: OutputFormat,
    pub continue_on_error: bool,
    pub quiet: bool,
    pub init: bool,
    pub print_version: bool,
    pub print_help: bool,
}

impl Default for CliArgs {
    fn default() -> Self {
        Self {
            db: PathBuf::from("./minidb.db"),
            mode: None,
            format: OutputFormat::default(),
            continue_on_error: false,
            quiet: false,
            init: false,
            print_version: false,
            print_help: false,
        }
    }
}

pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<CliArgs> {
    let argv: Vec<String> = args.into_iter().collect();
    let mut out = CliArgs::default();
    let mut i = 1; // skip program name
    while i < argv.len() {
        let a = &argv[i];
        match a.as_str() {
            "--repl" => {
                out.mode = Some(RunMode::Repl);
            }
            "--eval" => {
                let v = argv
                    .get(i + 1)
                    .ok_or_else(|| MiniDbError::cli("--eval requires a value"))?;
                out.mode = Some(RunMode::Eval(v.clone()));
                i += 1;
            }
            "--file" => {
                let v = argv
                    .get(i + 1)
                    .ok_or_else(|| MiniDbError::cli("--file requires a path"))?;
                out.mode = Some(RunMode::File(PathBuf::from(v)));
                i += 1;
            }
            "--db" => {
                let v = argv
                    .get(i + 1)
                    .ok_or_else(|| MiniDbError::cli("--db requires a value"))?;
                out.db = PathBuf::from(v);
                i += 1;
            }
            "--format" => {
                let v = argv
                    .get(i + 1)
                    .ok_or_else(|| MiniDbError::cli("--format requires a value"))?;
                out.format = OutputFormat::parse(v)?;
                i += 1;
            }
            "--continue-on-error" => {
                out.continue_on_error = true;
            }
            "--quiet" => {
                out.quiet = true;
            }
            "--init" => {
                out.init = true;
            }
            "--version" | "-V" => out.print_version = true,
            "--help" | "-h" => out.print_help = true,
            other if other.starts_with("--db=") => {
                out.db = PathBuf::from(other.trim_start_matches("--db="));
            }
            other if other.starts_with("--eval=") => {
                out.mode = Some(RunMode::Eval(
                    other.trim_start_matches("--eval=").to_string(),
                ));
            }
            other if other.starts_with("--file=") => {
                out.mode = Some(RunMode::File(PathBuf::from(
                    other.trim_start_matches("--file="),
                )));
            }
            other if other.starts_with("--format=") => {
                out.format = OutputFormat::parse(other.trim_start_matches("--format="))?;
            }
            other => return Err(MiniDbError::cli(format!("unknown argument: {}", other))),
        }
        i += 1;
    }
    Ok(out)
}

pub const HELP_TEXT: &str = "\
minidb -- a SQL-subset relational database in a single binary

USAGE:
    minidb [OPTIONS]

OPTIONS:
    --db <PATH>             Database directory (default: ./minidb.db)
    --eval <SQL>            Run a single SQL statement (or ;-separated list) and exit
    --file <PATH>           Run all SQL statements from a script file
    --repl                  Force interactive REPL mode
    --format <FORMAT>       Output format: table | csv | json (default: table)
    --continue-on-error     Continue executing a script after a statement fails
    --quiet                 Suppress per-statement progress summaries
    --init                  Create a fresh empty database directory and exit
    --version, -V           Print version and exit
    --help, -h              Print this help message

EXIT CODES:
    0  success
    1  SQL error
    2  CLI usage error
    3  storage / IO error
";

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        std::iter::once("minidb")
            .chain(items.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn parses_eval_and_db() {
        let a = parse_args(argv(&["--eval", "SELECT 1", "--db", "/tmp/x"])).unwrap();
        assert_eq!(a.db, PathBuf::from("/tmp/x"));
        assert_eq!(a.mode, Some(RunMode::Eval("SELECT 1".into())));
    }

    #[test]
    fn parses_format_eq_form() {
        let a = parse_args(argv(&["--format=json"])).unwrap();
        assert_eq!(a.format, OutputFormat::Json);
    }

    #[test]
    fn unknown_arg_fails() {
        assert!(parse_args(argv(&["--bogus"])).is_err());
    }

    #[test]
    fn flags_quiet_continue() {
        let a = parse_args(argv(&["--quiet", "--continue-on-error"])).unwrap();
        assert!(a.quiet);
        assert!(a.continue_on_error);
    }
}

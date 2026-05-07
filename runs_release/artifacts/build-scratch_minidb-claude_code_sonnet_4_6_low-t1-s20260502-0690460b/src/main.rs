mod catalog;
mod cli;
mod db;
mod error;
mod exec;
mod lexer;
mod parser;
mod plan;
mod storage;
mod txn;
mod types;

use clap::{Parser, ValueEnum};
use cli::{OutputFormat, Repl};
use db::Database;
use std::path::PathBuf;
use std::process;

#[derive(Debug, Clone, ValueEnum)]
enum Format {
    Table,
    Csv,
    Json,
}

impl From<Format> for OutputFormat {
    fn from(f: Format) -> Self {
        match f {
            Format::Table => OutputFormat::Table,
            Format::Csv => OutputFormat::Csv,
            Format::Json => OutputFormat::Json,
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "minidb",
    version,
    about = "An embeddable SQL-subset relational database"
)]
struct Args {
    /// Database file path
    #[arg(long, default_value = "./minidb.db")]
    db: PathBuf,

    /// Start interactive REPL
    #[arg(long)]
    repl: bool,

    /// Execute SQL string and exit
    #[arg(long, value_name = "SQL")]
    eval: Option<String>,

    /// Execute SQL script file and exit
    #[arg(long, value_name = "PATH")]
    file: Option<PathBuf>,

    /// Continue executing on error (only with --file)
    #[arg(long)]
    continue_on_error: bool,

    /// Output format
    #[arg(long, value_enum, default_value = "table")]
    format: Format,

    /// Suppress per-statement summary lines
    #[arg(long)]
    quiet: bool,

    /// Create a fresh empty database file and exit
    #[arg(long)]
    init: bool,
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args = Args::parse();

    if args.init {
        if args.db.exists() {
            eprintln!("Error: database '{}' already exists", args.db.display());
            process::exit(3);
        }
        match Database::open(&args.db) {
            Ok(_) => {
                eprintln!("Initialized empty database at '{}'", args.db.display());
                process::exit(0);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                process::exit(3);
            }
        }
    }

    let db = match Database::open(&args.db) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error opening database '{}': {}", args.db.display(), e);
            process::exit(e.exit_code());
        }
    };

    let format: OutputFormat = args.format.into();
    let mut repl = Repl::new(db, format, args.quiet);

    if let Some(sql) = args.eval {
        match repl.execute_batch(&sql, args.continue_on_error) {
            Ok(()) => process::exit(0),
            Err((e, _)) => {
                eprintln!("Error: {}", e);
                process::exit(e.exit_code());
            }
        }
    } else if let Some(path) = args.file {
        let sql = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", path.display(), e);
                process::exit(3);
            }
        };
        match repl.execute_batch(&sql, args.continue_on_error) {
            Ok(()) => process::exit(0),
            Err((e, _)) => {
                eprintln!("Error: {}", e);
                process::exit(e.exit_code());
            }
        }
    } else if args.repl || is_tty() {
        if let Err(e) = repl.run_interactive() {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    } else {
        // Read from stdin
        let mut sql = String::new();
        match std::io::Read::read_to_string(&mut std::io::stdin(), &mut sql) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error reading stdin: {}", e);
                process::exit(3);
            }
        }
        match repl.execute_batch(&sql, args.continue_on_error) {
            Ok(()) => process::exit(0),
            Err((e, _)) => {
                eprintln!("Error: {}", e);
                process::exit(e.exit_code());
            }
        }
    }
}

fn is_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

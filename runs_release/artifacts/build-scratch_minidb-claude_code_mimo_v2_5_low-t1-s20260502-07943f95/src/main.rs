pub mod catalog;
pub mod cli;
pub mod error;
pub mod exec;
pub mod lexer;
pub mod parser;
pub mod plan;
pub mod storage;
pub mod txn;
pub mod types;

use cli::MiniDbCli;
use error::{MiniDbError, Result};
use exec::OutputFormat;
use std::env;

const VERSION: &str = "0.1.0";

fn print_help() {
    eprintln!("minidb {}", VERSION);
    eprintln!("An embeddable SQL-subset relational database");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  minidb [OPTIONS]");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("  --db <PATH>           Database file path (default: ./minidb.db)");
    eprintln!("  --eval <SQL>          Execute SQL statement(s) and exit");
    eprintln!("  --file <PATH>         Read and execute SQL script");
    eprintln!("  --continue-on-error  Continue on error in file mode");
    eprintln!("  --repl                Force interactive REPL mode");
    eprintln!("  --format <FMT>        Output format: table, csv, json (default: table)");
    eprintln!("  --quiet               Suppress per-statement summary");
    eprintln!("  --init                Create empty database and exit");
    eprintln!("  --version             Print version");
    eprintln!("  --help                Print this help");
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        // Check if stdin is TTY
        if atty_is_tty() {
            // REPL mode
        } else {
            // Read from stdin
        }
    }

    let mut db_path = String::from("./minidb.db");
    let mut eval: Option<String> = None;
    let mut file: Option<String> = None;
    let mut format = OutputFormat::Table;
    let mut quiet = false;
    let mut init_only = false;
    let mut continue_on_error = false;
    let mut force_repl = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                i += 1;
                if i < args.len() {
                    db_path = args[i].clone();
                } else {
                    eprintln!("Error: --db requires a path argument");
                    return std::process::ExitCode::from(2);
                }
            }
            "--eval" => {
                i += 1;
                if i < args.len() {
                    eval = Some(args[i].clone());
                } else {
                    eprintln!("Error: --eval requires a SQL argument");
                    return std::process::ExitCode::from(2);
                }
            }
            "--file" => {
                i += 1;
                if i < args.len() {
                    file = Some(args[i].clone());
                } else {
                    eprintln!("Error: --file requires a path argument");
                    return std::process::ExitCode::from(2);
                }
            }
            "--format" => {
                i += 1;
                if i < args.len() {
                    match args[i].as_str() {
                        "table" => format = OutputFormat::Table,
                        "csv" => format = OutputFormat::Csv,
                        "json" => format = OutputFormat::Json,
                        _ => {
                            eprintln!("Error: Unknown format '{}'", args[i]);
                            return std::process::ExitCode::from(2);
                        }
                    }
                } else {
                    eprintln!("Error: --format requires a format argument");
                    return std::process::ExitCode::from(2);
                }
            }
            "--quiet" => quiet = true,
            "--init" => init_only = true,
            "--continue-on-error" => continue_on_error = true,
            "--repl" => force_repl = true,
            "--version" => {
                eprintln!("minidb {}", VERSION);
                return std::process::ExitCode::from(0);
            }
            "--help" => {
                print_help();
                return std::process::ExitCode::from(0);
            }
            _ => {
                eprintln!("Error: Unknown argument '{}'", args[i]);
                eprintln!("Use --help for usage information");
                return std::process::ExitCode::from(2);
            }
        }
        i += 1;
    }

    let cli = MiniDbCli::new(&db_path, format, quiet, init_only, continue_on_error);

    match cli.run(eval.as_deref(), file.as_deref()) {
        Ok(()) => std::process::ExitCode::from(0),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::ExitCode::from(1)
        }
    }
}

fn atty_is_tty() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::isatty(libc::STDIN_FILENO) != 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

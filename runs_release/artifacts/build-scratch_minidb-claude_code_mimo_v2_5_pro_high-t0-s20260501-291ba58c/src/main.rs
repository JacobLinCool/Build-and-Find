mod catalog;
mod cli;
mod error;
mod exec;
mod lexer;
mod parser;
mod plan;
mod storage;
mod txn;
mod types;

use cli::{Cli, OutputFormat};
use error::MiniDbError;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args: Vec<String> = std::env::args().collect();
    let mut db_path = PathBuf::from("./minidb.db");
    let mut format = OutputFormat::Table;
    let mut quiet = false;
    let mut repl = false;
    let mut eval_sql: Option<String> = None;
    let mut file_path: Option<PathBuf> = None;
    let mut continue_on_error = false;
    let mut do_init = false;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --db requires a path argument");
                    process::exit(2);
                }
                db_path = PathBuf::from(&args[i]);
            }
            "--format" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --format requires an argument");
                    process::exit(2);
                }
                format = match args[i].as_str() {
                    "table" => OutputFormat::Table,
                    "csv" => OutputFormat::Csv,
                    "json" => OutputFormat::Json,
                    _ => {
                        eprintln!("Error: unknown format '{}'", args[i]);
                        process::exit(2);
                    }
                };
            }
            "--quiet" => quiet = true,
            "--repl" => repl = true,
            "--eval" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --eval requires a SQL argument");
                    process::exit(2);
                }
                eval_sql = Some(args[i].clone());
            }
            "--file" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --file requires a path argument");
                    process::exit(2);
                }
                file_path = Some(PathBuf::from(&args[i]));
            }
            "--continue-on-error" => continue_on_error = true,
            "--init" => do_init = true,
            "--version" => {
                println!("minidb {}", env!("CARGO_PKG_VERSION"));
                process::exit(0);
            }
            "--help" | "-h" => {
                print_help();
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                eprintln!("Error: unknown option '{arg}'");
                process::exit(2);
            }
            _ => {
                eprintln!("Error: unexpected argument '{}'", args[i]);
                process::exit(2);
            }
        }
        i += 1;
    }

    // Handle --init
    if do_init {
        match exec::Executor::create(&db_path) {
            Ok(_) => {
                if !quiet {
                    eprintln!("Database initialized: {}", db_path.display());
                }
                process::exit(0);
            }
            Err(e) => {
                eprintln!("Error: {e}");
                process::exit(3);
            }
        }
    }

    // Determine mode
    let is_tty = io::stdin().is_terminal();
    let should_repl = repl || (is_tty && eval_sql.is_none() && file_path.is_none());

    let cli = Cli::new(db_path.clone(), format, quiet, should_repl);

    // Open or create database
    let mut executor = if db_path.exists() {
        match exec::Executor::open(&db_path) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Error opening database: {e}");
                process::exit(3);
            }
        }
    } else {
        match exec::Executor::create(&db_path) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Error creating database: {e}");
                process::exit(3);
            }
        }
    };

    // Execute --eval
    if let Some(sql) = eval_sql {
        match cli.execute_batch(&mut executor, &sql, false) {
            Ok(()) => process::exit(0),
            Err(MiniDbError::Runtime(msg)) if msg == "exit" => process::exit(0),
            Err(MiniDbError::Io(_)) | Err(MiniDbError::Storage(_)) | Err(MiniDbError::Wal(_)) => {
                process::exit(3)
            }
            Err(MiniDbError::Parse { .. })
            | Err(MiniDbError::Syntax(_))
            | Err(MiniDbError::Type(_))
            | Err(MiniDbError::Constraint(_))
            | Err(MiniDbError::Catalog(_))
            | Err(MiniDbError::Runtime(_)) => process::exit(1),
            Err(_) => process::exit(1),
        }
    }

    // Execute --file
    if let Some(path) = file_path {
        let sql = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading file: {e}");
                process::exit(2);
            }
        };
        match cli.execute_batch(&mut executor, &sql, continue_on_error) {
            Ok(()) => process::exit(0),
            Err(MiniDbError::Io(_)) | Err(MiniDbError::Storage(_)) | Err(MiniDbError::Wal(_)) => {
                process::exit(3)
            }
            Err(_) => process::exit(1),
        }
    }

    // REPL mode
    if should_repl {
        match cli.run() {
            Ok(()) => process::exit(0),
            Err(e) => {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
    }

    // If nothing else, try reading from stdin
    let mut sql = String::new();
    match io::stdin().read_line(&mut sql) {
        Ok(0) => process::exit(0),
        Ok(_) => match cli.execute_batch(&mut executor, &sql.trim(), false) {
            Ok(()) => process::exit(0),
            Err(MiniDbError::Io(_)) | Err(MiniDbError::Storage(_)) | Err(MiniDbError::Wal(_)) => {
                process::exit(3)
            }
            Err(_) => process::exit(1),
        },
        Err(e) => {
            eprintln!("Error reading stdin: {e}");
            process::exit(3);
        }
    }
}

fn print_help() {
    let help = r#"minidb - an embeddable SQL-subset relational database

Usage: minidb [OPTIONS]

Options:
  --db <PATH>              Database file path (default: ./minidb.db)
  --format <KIND>          Output format: table, csv, json (default: table)
  --quiet                  Suppress per-statement summary lines
  --repl                   Force interactive REPL mode
  --eval "<SQL>"           Execute SQL and exit
  --file <PATH>            Execute SQL script from file
  --continue-on-error      Continue executing script on error
  --init                   Create a fresh empty database and exit
  --version                Print version and exit
  --help, -h               Print this help message

SQL Statements:
  CREATE TABLE, DROP TABLE, CREATE INDEX, DROP INDEX
  INSERT, SELECT, UPDATE, DELETE
  BEGIN, COMMIT, ROLLBACK

Meta-Commands:
  .schema    Show all table schemas
  .tables    List all tables
  .indexes   List all indexes
  .exit      Exit the REPL
  .format    Set output format

Examples:
  minidb                           # Start REPL
  minidb --db my.db                # Use specific database file
  minidb --eval "SELECT 1+1"       # Execute SQL and exit
  minidb --file script.sql         # Execute SQL script
"#;
    println!("{help}");
}

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

use cli::MiniDbCli;
use error::{MiniDbError, Result};
use std::env;
use std::path::PathBuf;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    env_logger::init();

    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        // Check if stdin is a TTY
        if atty::is(atty::Stream::Stdin) {
            run_repl(&default_db_path());
        } else {
            eprintln!("Usage: minidb [OPTIONS]");
            eprintln!("  --db <PATH>          Database file path (default: ./minidb.db)");
            eprintln!("  --eval \"<SQL>\"       Execute SQL statement(s)");
            eprintln!("  --file <PATH>        Execute SQL script file");
            eprintln!("  --continue-on-error  Continue on errors in batch mode");
            eprintln!("  --repl               Start interactive REPL");
            eprintln!("  --init               Create fresh database and exit");
            eprintln!("  --format <FMT>       Output format: table, csv, json");
            eprintln!("  --quiet              Suppress summary output");
            eprintln!("  --version            Print version");
            eprintln!("  --help               Print this help");
            std::process::exit(2);
        }
        return;
    }

    let mut db_path = default_db_path();
    let mut eval_sql: Option<String> = None;
    let mut file_path: Option<String> = None;
    let mut continue_on_error = false;
    let mut force_repl = false;
    let mut init_only = false;
    let mut quiet = false;
    let mut format: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                i += 1;
                if i < args.len() {
                    db_path = PathBuf::from(&args[i]);
                } else {
                    eprintln!("Error: --db requires a path");
                    std::process::exit(2);
                }
            }
            "--eval" => {
                i += 1;
                if i < args.len() {
                    eval_sql = Some(args[i].clone());
                } else {
                    eprintln!("Error: --eval requires a SQL string");
                    std::process::exit(2);
                }
            }
            "--file" => {
                i += 1;
                if i < args.len() {
                    file_path = Some(args[i].clone());
                } else {
                    eprintln!("Error: --file requires a path");
                    std::process::exit(2);
                }
            }
            "--continue-on-error" => {
                continue_on_error = true;
            }
            "--repl" => {
                force_repl = true;
            }
            "--init" => {
                init_only = true;
            }
            "--format" => {
                i += 1;
                if i < args.len() {
                    format = Some(args[i].clone());
                } else {
                    eprintln!("Error: --format requires a format (table, csv, json)");
                    std::process::exit(2);
                }
            }
            "--quiet" => {
                quiet = true;
            }
            "--version" => {
                println!("minidb {}", VERSION);
                return;
            }
            "--help" => {
                print_usage();
                return;
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let mut cli = MiniDbCli::new(db_path.clone(), quiet);

    if let Some(fmt) = format {
        match fmt.as_str() {
            "csv" => cli.set_format(cli::OutputFormat::Csv),
            "json" => cli.set_format(cli::OutputFormat::Json),
            "table" => cli.set_format(cli::OutputFormat::Table),
            _ => {
                eprintln!("Error: unknown format '{}'", fmt);
                std::process::exit(2);
            }
        }
    }

    if init_only {
        match cli.init_db() {
            Ok(()) => {
                if !quiet {
                    eprintln!("Database initialized at {:?}", db_path);
                }
            }
            Err(e) => {
                eprintln!("Error initializing database: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    if let Some(sql) = eval_sql {
        match cli.open() {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Error opening database: {}", e);
                std::process::exit(3);
            }
        }
        match cli.run_eval(&sql) {
            Ok(()) => std::process::exit(0),
            Err(_) => std::process::exit(1),
        }
    }

    if let Some(path) = file_path {
        match cli.open() {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Error opening database: {}", e);
                std::process::exit(3);
            }
        }
        match cli.run_file(&path, continue_on_error) {
            Ok(()) => std::process::exit(0),
            Err(_) => std::process::exit(1),
        }
    }

    // Default: REPL
    if atty::is(atty::Stream::Stdin) || force_repl {
        match cli.open() {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Error opening database: {}", e);
                std::process::exit(3);
            }
        }
        match cli.run_repl() {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        eprintln!("Usage: minidb [OPTIONS]");
        eprintln!("Try 'minidb --help' for more information.");
        std::process::exit(2);
    }
}

fn default_db_path() -> PathBuf {
    PathBuf::from("./minidb.db")
}

fn run_repl(db_path: &PathBuf) {
    let mut cli = MiniDbCli::new(db_path.clone(), false);
    if let Err(e) = cli.open() {
        eprintln!("Error opening database: {}", e);
        std::process::exit(3);
    }
    if let Err(e) = cli.run_repl() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn print_usage() {
    println!("minidb {}", VERSION);
    println!();
    println!("An embeddable SQL-subset relational database");
    println!();
    println!("USAGE:");
    println!("    minidb [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    --db <PATH>              Database file path (default: ./minidb.db)");
    println!("    --eval \"<SQL>\"           Execute SQL statement(s) and exit");
    println!("    --file <PATH>            Execute SQL script file");
    println!("    --continue-on-error      Continue on errors in batch mode");
    println!("    --repl                   Start interactive REPL");
    println!("    --init                   Create fresh database and exit");
    println!("    --format <FMT>           Output format: table (default), csv, json");
    println!("    --quiet                  Suppress summary output");
    println!("    --version                Print version");
    println!("    --help                   Print this help");
    println!();
    println!("SQL SUBSET:");
    println!("    CREATE TABLE, DROP TABLE, CREATE INDEX, DROP INDEX");
    println!("    INSERT, SELECT, UPDATE, DELETE");
    println!("    BEGIN, COMMIT, ROLLBACK");
    println!("    Expressions: +, -, *, /, ||, =, <>, <, <=, >, >=");
    println!("    Functions: LENGTH, LOWER, UPPER, SUBSTR, ABS, COALESCE");
    println!("    SELECT: *, JOIN, WHERE, ORDER BY, LIMIT, OFFSET");
    println!();
    println!("META COMMANDS:");
    println!("    .schema    Show table schemas");
    println!("    .tables    List all tables");
    println!("    .indexes   List all indexes");
    println!("    .format X  Set output format (table, csv, json)");
    println!("    .exit      Exit the REPL");
}

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

use clap::{Arg, ArgAction, Command};
use cli::{CliSession, OutputFormat};
use exec::engine::Executor;
use storage::StorageEngine;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let matches = Command::new("minidb")
        .version(VERSION)
        .about("An embeddable SQL-subset relational database")
        .arg(
            Arg::new("db")
                .long("db")
                .value_name("PATH")
                .default_value("./minidb.db")
                .help("Database file path"),
        )
        .arg(
            Arg::new("eval")
                .long("eval")
                .value_name("SQL")
                .help("Execute SQL statement(s) and exit"),
        )
        .arg(
            Arg::new("file")
                .long("file")
                .short('f')
                .value_name("PATH")
                .help("Execute SQL script file and exit"),
        )
        .arg(
            Arg::new("repl")
                .long("repl")
                .action(ArgAction::SetTrue)
                .help("Force REPL mode"),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .value_name("FORMAT")
                .default_value("table")
                .help("Output format: table, csv, json"),
        )
        .arg(
            Arg::new("quiet")
                .long("quiet")
                .short('q')
                .action(ArgAction::SetTrue)
                .help("Suppress per-statement summary lines"),
        )
        .arg(
            Arg::new("continue-on-error")
                .long("continue-on-error")
                .action(ArgAction::SetTrue)
                .help("Continue executing after errors (for --file mode)"),
        )
        .arg(
            Arg::new("init")
                .long("init")
                .action(ArgAction::SetTrue)
                .help("Create a fresh empty database file and exit"),
        )
        .get_matches();

    let db_path = matches.get_one::<String>("db").unwrap();
    let format_str = matches.get_one::<String>("format").unwrap();
    let quiet = matches.get_flag("quiet");
    let continue_on_error = matches.get_flag("continue-on-error");

    let format = match format_str.as_str() {
        "csv" => OutputFormat::Csv,
        "json" => OutputFormat::Json,
        _ => OutputFormat::Table,
    };

    if matches.get_flag("init") {
        match StorageEngine::init_fresh(db_path) {
            Ok(_) => {
                eprintln!("Database initialized: {}", db_path);
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("Error initializing database: {}", e);
                std::process::exit(3);
            }
        }
    }

    let engine = match StorageEngine::open(db_path) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error opening database '{}': {}", db_path, e);
            std::process::exit(3);
        }
    };

    let executor = Executor::new(engine);
    let mut session = CliSession::new(executor, format, quiet);

    if let Some(sql) = matches.get_one::<String>("eval") {
        let code = cli::run_eval(&mut session, sql);
        std::process::exit(code);
    }

    if let Some(file_path) = matches.get_one::<String>("file") {
        let code = cli::run_file(&mut session, file_path, continue_on_error);
        std::process::exit(code);
    }

    // REPL mode
    cli::run_repl(&mut session);
    std::process::exit(0);
}

use clap::{Arg, ArgAction, Command};
use minidb::cli::format::OutputFormat;
use minidb::exec::Database;
use std::path::PathBuf;
use std::process;

fn main() {
    env_logger::init();

    let matches = Command::new("minidb")
        .version(env!("CARGO_PKG_VERSION"))
        .about("MiniDB -- embeddable SQL-subset relational database")
        .arg(
            Arg::new("db")
                .long("db")
                .value_name("PATH")
                .help("Database file path")
                .default_value("./minidb.db"),
        )
        .arg(
            Arg::new("eval")
                .long("eval")
                .short('e')
                .value_name("SQL")
                .help("Execute SQL and exit"),
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
                .help("Force interactive REPL mode"),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .value_name("FORMAT")
                .help("Output format: table, csv, json")
                .default_value("table"),
        )
        .arg(
            Arg::new("quiet")
                .long("quiet")
                .short('q')
                .action(ArgAction::SetTrue)
                .help("Suppress per-statement summary lines"),
        )
        .arg(
            Arg::new("init")
                .long("init")
                .action(ArgAction::SetTrue)
                .help("Initialize a fresh empty database file and exit"),
        )
        .arg(
            Arg::new("continue-on-error")
                .long("continue-on-error")
                .action(ArgAction::SetTrue)
                .help("Continue executing after an error (for --file mode)"),
        )
        .get_matches();

    let db_path = PathBuf::from(matches.get_one::<String>("db").unwrap());
    let quiet = matches.get_flag("quiet");
    let continue_on_error = matches.get_flag("continue-on-error");

    let format_str = matches.get_one::<String>("format").unwrap();
    let fmt = match OutputFormat::from_str(format_str) {
        Some(f) => f,
        None => {
            eprintln!(
                "Error: unknown format '{}'. Use: table, csv, json",
                format_str
            );
            process::exit(2);
        }
    };

    // --init: create fresh DB and exit
    if matches.get_flag("init") {
        if db_path.exists() {
            std::fs::remove_file(&db_path).ok();
            let catalog_path = db_path.with_extension("db.catalog");
            std::fs::remove_file(&catalog_path).ok();
            let wal_path = db_path.with_extension("db-wal");
            std::fs::remove_file(&wal_path).ok();
        }
        match Database::open(&db_path) {
            Ok(_) => {
                if !quiet {
                    eprintln!("ok database initialized at {}", db_path.display());
                }
                process::exit(0);
            }
            Err(e) => {
                eprintln!("Error initializing database: {}", e);
                process::exit(3);
            }
        }
    }

    // Open database
    let mut db = match Database::open(&db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error opening database: {}", e);
            process::exit(3);
        }
    };

    // --eval mode
    if let Some(sql) = matches.get_one::<String>("eval") {
        let rc = minidb::cli::exec_sql(&mut db, sql, &fmt, quiet, continue_on_error);
        process::exit(rc);
    }

    // --file mode
    if let Some(file_path) = matches.get_one::<String>("file") {
        let path = std::path::Path::new(file_path);
        let rc = minidb::cli::exec_file(&mut db, path, &fmt, quiet, continue_on_error);
        process::exit(rc);
    }

    // REPL mode
    let mut repl = minidb::cli::repl::Repl::new(fmt, quiet);
    let rc = repl.run(&mut db);
    process::exit(rc);
}

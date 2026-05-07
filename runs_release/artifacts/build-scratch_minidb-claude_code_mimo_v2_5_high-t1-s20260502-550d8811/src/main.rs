use minidb::cli;
use minidb::error::exit_code_for;
use std::env;
use std::io::BufRead;
use std::path::PathBuf;
use std::process;

const VERSION: &str = "0.1.0";

fn main() {
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    let mut db_path: Option<PathBuf> = None;
    let mut eval_sql: Option<String> = None;
    let mut file_path: Option<PathBuf> = None;
    let mut continue_on_error = false;
    let mut quiet = false;
    let mut format: Option<String> = None;
    let mut force_repl = false;
    let mut init_mode = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--version" | "-v" => {
                println!("minidb {}", VERSION);
                process::exit(0);
            }
            "--help" | "-h" => {
                print_help();
                process::exit(0);
            }
            "--init" => {
                init_mode = true;
                i += 1;
            }
            "--db" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --db requires a path");
                    process::exit(2);
                }
                db_path = Some(PathBuf::from(&args[i]));
                i += 1;
            }
            "--eval" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --eval requires a SQL string");
                    process::exit(2);
                }
                eval_sql = Some(args[i].clone());
                i += 1;
            }
            "--file" | "-f" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --file requires a path");
                    process::exit(2);
                }
                file_path = Some(PathBuf::from(&args[i]));
                i += 1;
            }
            "--continue-on-error" => {
                continue_on_error = true;
                i += 1;
            }
            "--format" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --format requires table, csv, or json");
                    process::exit(2);
                }
                format = Some(args[i].clone());
                i += 1;
            }
            "--quiet" | "-q" => {
                quiet = true;
                i += 1;
            }
            "--repl" => {
                force_repl = true;
                i += 1;
            }
            other if other.starts_with('-') && !other.starts_with("--") => {
                // Handle combined short flags
                for ch in other[1..].chars() {
                    match ch {
                        'q' => quiet = true,
                        'h' => {
                            print_help();
                            process::exit(0);
                        }
                        'v' => {
                            println!("minidb {}", VERSION);
                            process::exit(0);
                        }
                        'f' => {
                            i += 1;
                            if i >= args.len() {
                                eprintln!("Error: -f requires a path");
                                process::exit(2);
                            }
                            file_path = Some(PathBuf::from(&args[i]));
                        }
                        _ => {
                            eprintln!("Error: unknown flag '-{}'", ch);
                            process::exit(2);
                        }
                    }
                }
                i += 1;
            }
            other => {
                // Treat as db path
                db_path = Some(PathBuf::from(other));
                i += 1;
            }
        }
    }

    let db_path = db_path.unwrap_or_else(|| PathBuf::from("./minidb.db"));

    // Init mode
    if init_mode {
        if let Err(e) = cli::init_db(&db_path) {
            eprintln!("Error: {}", e);
            process::exit(3);
        }
        process::exit(0);
    }

    // Open database
    let mut db = match cli::MiniDb::open(&db_path, quiet) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(exit_code_for(&e));
        }
    };

    // Set format
    if let Some(ref fmt) = format {
        match fmt.as_str() {
            "table" => db.set_format(cli::OutputFormat::Table),
            "csv" => db.set_format(cli::OutputFormat::Csv),
            "json" => db.set_format(cli::OutputFormat::Json),
            _ => {
                eprintln!("Error: unknown format '{}'. Use table, csv, or json", fmt);
                process::exit(2);
            }
        }
    }

    // Execute based on mode
    if let Some(sql) = eval_sql {
        match cli::run_eval(&mut db, &sql) {
            Ok(()) => process::exit(0),
            Err(e) => {
                eprintln!("Error: {}", e);
                process::exit(exit_code_for(&e));
            }
        }
    } else if let Some(path) = file_path {
        match cli::run_file(&mut db, &path, continue_on_error) {
            Ok(()) => process::exit(0),
            Err(e) => {
                eprintln!("Error: {}", e);
                process::exit(exit_code_for(&e));
            }
        }
    } else {
        // REPL mode - check if stdin is a TTY or --repl was passed
        let is_tty = atty_is_tty();
        if is_tty || force_repl {
            match cli::run_repl(&mut db) {
                Ok(()) => process::exit(0),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(exit_code_for(&e));
                }
            }
        } else {
            // Read from stdin (pipe)
            let stdin = std::io::stdin();
            let mut sql = String::new();
            for line in stdin.lock().lines() {
                match line {
                    Ok(l) => {
                        sql.push_str(&l);
                        sql.push('\n');
                    }
                    Err(e) => {
                        eprintln!("Error reading stdin: {}", e);
                        process::exit(3);
                    }
                }
            }
            match cli::run_eval(&mut db, &sql) {
                Ok(()) => process::exit(0),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(exit_code_for(&e));
                }
            }
        }
    }
}

fn atty_is_tty() -> bool {
    unsafe { libc_isatty(0) != 0 }
}

extern "C" {
    #[link_name = "isatty"]
    fn libc_isatty(fd: i32) -> i32;
}

fn print_help() {
    println!(
        "minidb {} - An embeddable SQL-subset relational database",
        VERSION
    );
    println!();
    println!("USAGE:");
    println!("  minidb [OPTIONS] [--db <PATH>]");
    println!();
    println!("OPTIONS:");
    println!("  --db <PATH>              Database file path (default: ./minidb.db)");
    println!("  --eval \"<SQL>\"            Execute SQL and exit");
    println!("  --file <PATH>            Execute SQL file and exit");
    println!("  --continue-on-error     Continue executing file on error");
    println!("  --format <FMT>           Output format: table, csv, json");
    println!("  --quiet, -q              Suppress per-statement summary");
    println!("  --repl                   Force interactive REPL mode");
    println!("  --init                   Create empty database and exit");
    println!("  --version, -v            Print version");
    println!("  --help, -h               Print this help");
    println!();
    println!("META-COMMANDS (REPL only):");
    println!("  .schema                  Show CREATE TABLE statements");
    println!("  .tables                  List all tables");
    println!("  .indexes                 List all indexes");
    println!("  .format <kind>           Set output format");
    println!("  .exit                    Exit the REPL");
    println!();
    println!("SQL SUPPORT:");
    println!("  CREATE TABLE, DROP TABLE, CREATE INDEX, DROP INDEX");
    println!("  INSERT, SELECT, UPDATE, DELETE");
    println!("  BEGIN, COMMIT, ROLLBACK");
    println!("  Column types: INT, BIGINT, REAL, TEXT, BOOL");
    println!("  Expressions: +, -, *, /, %, ||, =, <>, <, <=, >, >=, IS NULL, IS NOT NULL");
    println!("  Functions: LENGTH, LOWER, UPPER, SUBSTR, ABS, COALESCE");
}

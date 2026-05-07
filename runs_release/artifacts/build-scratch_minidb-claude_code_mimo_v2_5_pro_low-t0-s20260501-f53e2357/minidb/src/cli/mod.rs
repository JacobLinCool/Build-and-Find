use std::io::{self, BufRead, Write};

use crate::catalog::Catalog;
use crate::error::Result;
use crate::exec::{Engine, ExecResult};
use crate::parser::Parser;
use crate::storage::Storage;
use crate::types::Value;

pub struct CliArgs {
    pub db_path: String,
    pub repl: bool,
    pub eval: Option<String>,
    pub file: Option<String>,
    pub continue_on_error: bool,
    pub format: OutputFormat,
    pub quiet: bool,
    pub init: bool,
    pub version: bool,
    pub help: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl CliArgs {
    pub fn parse(args: &[String]) -> std::result::Result<Self, String> {
        let mut result = CliArgs {
            db_path: "./minidb.db".into(),
            repl: false,
            eval: None,
            file: None,
            continue_on_error: false,
            format: OutputFormat::Table,
            quiet: false,
            init: false,
            version: false,
            help: false,
        };

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--db" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--db requires a path".into());
                    }
                    result.db_path = args[i].clone();
                }
                "--repl" => result.repl = true,
                "--eval" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--eval requires a SQL string".into());
                    }
                    result.eval = Some(args[i].clone());
                }
                "--file" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--file requires a path".into());
                    }
                    result.file = Some(args[i].clone());
                }
                "--continue-on-error" => result.continue_on_error = true,
                "--format" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--format requires a value (table, csv, json)".into());
                    }
                    result.format = match args[i].as_str() {
                        "table" => OutputFormat::Table,
                        "csv" => OutputFormat::Csv,
                        "json" => OutputFormat::Json,
                        other => return Err(format!("unknown format '{}'", other)),
                    };
                }
                "--quiet" => result.quiet = true,
                "--init" => result.init = true,
                "--version" => result.version = true,
                "--help" | "-h" => result.help = true,
                other if other.starts_with('-') => {
                    return Err(format!("unknown option '{}'", other));
                }
                _ => {
                    return Err(format!("unexpected argument '{}'", args[i]));
                }
            }
            i += 1;
        }

        // If stdin is a TTY and no eval/file, default to REPL
        if result.eval.is_none() && result.file.is_none() {
            result.repl = true;
        }

        Ok(result)
    }
}

pub fn run(args: CliArgs) -> i32 {
    if args.version {
        eprintln!("minidb {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }

    if args.help {
        print_help();
        return 0;
    }

    if args.init {
        match init_database(&args.db_path) {
            Ok(()) => {
                if !args.quiet {
                    eprintln!("ok database initialized: {}", args.db_path);
                }
                return 0;
            }
            Err(e) => {
                eprintln!("error: {}", e);
                return 3;
            }
        }
    }

    let db_path = std::path::Path::new(&args.db_path);
    let mut storage = match Storage::open(db_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {}", e);
            return 3;
        }
    };

    let catalog = match Catalog::load(&mut storage.pm) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {}", e);
            return 3;
        }
    };

    let mut engine = Engine::new(storage, catalog);

    // Replay WAL
    if let Err(e) = engine.txn.replay_wal(&mut engine.storage) {
        log::warn!("WAL replay warning: {}", e);
    }

    if let Some(sql) = args.eval {
        return execute_sql(&mut engine, &sql, &args.format, args.quiet);
    }

    if let Some(path) = args.file {
        return execute_file(
            &mut engine,
            &path,
            &args.format,
            args.quiet,
            args.continue_on_error,
        );
    }

    if args.repl {
        return run_repl(&mut engine, &args.format, args.quiet);
    }

    0
}

fn init_database(path: &str) -> Result<()> {
    let db_path = std::path::Path::new(path);
    if db_path.exists() {
        std::fs::remove_file(db_path)?;
    }
    let wal_path = db_path.with_extension("wal");
    if wal_path.exists() {
        std::fs::remove_file(&wal_path)?;
    }
    let mut storage = Storage::open(db_path)?;
    let _catalog = Catalog::load(&mut storage.pm)?;
    storage.flush()?;
    Ok(())
}

fn execute_sql(engine: &mut Engine, sql: &str, format: &OutputFormat, quiet: bool) -> i32 {
    let stmts = match Parser::parse_all(sql) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    let mut last_code = 0;
    for stmt in stmts {
        match engine.execute_statement(stmt) {
            Ok(result) => {
                let affected = print_result(&result, format, quiet);
                if let Some(code) = affected {
                    last_code = code;
                }
            }
            Err(e) => {
                eprintln!("error: {}", e);
                last_code = 1;
            }
        }
    }
    last_code
}

fn execute_file(
    engine: &mut Engine,
    path: &str,
    format: &OutputFormat,
    quiet: bool,
    continue_on_error: bool,
) -> i32 {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot read file '{}': {}", path, e);
            return 3;
        }
    };

    let stmts = match Parser::parse_all(&content) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };

    for stmt in stmts {
        match engine.execute_statement(stmt) {
            Ok(result) => {
                print_result(&result, format, quiet);
            }
            Err(e) => {
                eprintln!("error: {}", e);
                if !continue_on_error {
                    return 1;
                }
            }
        }
    }
    0
}

fn run_repl(engine: &mut Engine, format: &OutputFormat, quiet: bool) -> i32 {
    let stdin = io::stdin();
    let mut format = format.clone();
    let mut buffer = String::new();

    loop {
        eprint!("minidb> ");
        io::stderr().flush().ok();

        buffer.clear();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => {
                eprintln!("error reading input: {}", e);
                return 3;
            }
        }

        buffer.push_str(&line);

        // Read until we have a semicolon
        while !buffer.trim_end().ends_with(';') && !buffer.trim().is_empty() {
            eprint!("    ...> ");
            io::stderr().flush().ok();
            line.clear();
            match stdin.lock().read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
            buffer.push_str(&line);
        }

        let trimmed = buffer.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Handle meta-commands that don't need semicolons
        if trimmed.starts_with('.') && !trimmed.ends_with(';') {
            // Add semicolon for parsing
            buffer.push(';');
        }

        let stmts = match Parser::parse_all(&buffer) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {}", e);
                continue;
            }
        };

        for stmt in stmts {
            match engine.execute_statement(stmt) {
                Ok(ExecResult::Exit) => return 0,
                Ok(ExecResult::SetFormat(kind)) => {
                    format = match kind.to_lowercase().as_str() {
                        "csv" => OutputFormat::Csv,
                        "json" => OutputFormat::Json,
                        _ => OutputFormat::Table,
                    };
                    if !quiet {
                        eprintln!("ok format set to {:?}", format);
                    }
                }
                Ok(result) => {
                    print_result(&result, &format, quiet);
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                }
            }
        }
    }

    0
}

fn print_result(result: &ExecResult, format: &OutputFormat, quiet: bool) -> Option<i32> {
    match result {
        ExecResult::Rows { columns, rows } => {
            match format {
                OutputFormat::Table => print_table(columns, rows),
                OutputFormat::Csv => print_csv(columns, rows),
                OutputFormat::Json => print_json(columns, rows),
            }
            if !quiet {
                eprintln!(
                    "ok {} row{}",
                    rows.len(),
                    if rows.len() == 1 { "" } else { "s" }
                );
            }
            None
        }
        ExecResult::RowsAffected(n) => {
            if !quiet {
                eprintln!("ok {} row{} affected", n, if *n == 1 { "" } else { "s" });
            }
            None
        }
        ExecResult::Message(msg) => {
            if !msg.is_empty() {
                println!("{}", msg);
            }
            if !quiet {
                eprintln!("ok");
            }
            None
        }
        ExecResult::SetFormat(_) => None,
        ExecResult::Exit => None,
    }
}

fn print_table(columns: &[String], rows: &[Vec<Value>]) {
    if columns.is_empty() && rows.is_empty() {
        return;
    }

    // Calculate column widths
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(format!("{}", val).len());
            }
        }
    }

    // Print header
    for (i, col) in columns.iter().enumerate() {
        if i > 0 {
            print!(" | ");
        }
        print!(
            "{:<width$}",
            col,
            width = widths.get(i).copied().unwrap_or(0)
        );
    }
    if !columns.is_empty() {
        println!();
        // Print separator
        for (i, w) in widths.iter().enumerate() {
            if i > 0 {
                print!("-+-");
            }
            print!("{}", "-".repeat(*w));
        }
        println!();
    }

    // Print rows
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i > 0 {
                print!(" | ");
            }
            let w = widths.get(i).copied().unwrap_or(0);
            print!("{:<width$}", val, width = w);
        }
        println!();
    }
}

fn print_csv(columns: &[String], rows: &[Vec<Value>]) {
    // Header
    for (i, col) in columns.iter().enumerate() {
        if i > 0 {
            print!(",");
        }
        print_csv_field(col);
    }
    println!();

    // Rows
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i > 0 {
                print!(",");
            }
            print_csv_field(&format!("{}", val));
        }
        println!();
    }
}

fn print_csv_field(field: &str) {
    if field.contains(',') || field.contains('"') || field.contains('\n') {
        print!("\"{}\"", field.replace('"', "\"\""));
    } else {
        print!("{}", field);
    }
}

fn print_json(columns: &[String], rows: &[Vec<Value>]) {
    let json_rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let mut map = serde_json::Map::new();
            for (i, val) in row.iter().enumerate() {
                let key = columns
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("col_{}", i));
                let json_val = match val {
                    Value::Int(n) => serde_json::Value::Number((*n).into()),
                    Value::Real(n) => serde_json::Number::from_f64(*n)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null),
                    Value::Text(s) => serde_json::Value::String(s.clone()),
                    Value::Bool(b) => serde_json::Value::Bool(*b),
                    Value::Null => serde_json::Value::Null,
                };
                map.insert(key, json_val);
            }
            serde_json::Value::Object(map)
        })
        .collect();

    match serde_json::to_string(&json_rows) {
        Ok(s) => println!("{}", s),
        Err(_) => println!("[]"),
    }
}

fn print_help() {
    eprintln!(
        "minidb - an embeddable SQL-subset relational database\n\
         \n\
         USAGE:\n\
         \x20 minidb [OPTIONS]\n\
         \n\
         OPTIONS:\n\
         \x20 --db <PATH>            Database file path (default: ./minidb.db)\n\
         \x20 --repl                 Force interactive REPL mode\n\
         \x20 --eval <SQL>           Execute SQL statement(s) and exit\n\
         \x20 --file <PATH>          Execute SQL script file and exit\n\
         \x20 --continue-on-error    Continue executing script on error\n\
         \x20 --format <FMT>         Output format: table, csv, json (default: table)\n\
         \x20 --quiet                Suppress per-statement summary lines\n\
         \x20 --init                 Create a fresh empty database and exit\n\
         \x20 --version              Print version and exit\n\
         \x20 --help, -h             Print this help and exit\n\
         \n\
         META-COMMANDS (in REPL):\n\
         \x20 .schema                Show all table and index definitions\n\
         \x20 .tables                List all tables\n\
         \x20 .indexes               List all indexes\n\
         \x20 .format <kind>         Set output format\n\
         \x20 .exit                  Exit the REPL"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_args_default() {
        let args = CliArgs::parse(&[]).unwrap();
        assert_eq!(args.db_path, "./minidb.db");
        assert!(args.repl);
    }

    #[test]
    fn test_parse_args_eval() {
        let args = CliArgs::parse(&["--eval".into(), "SELECT 1".into()]).unwrap();
        assert_eq!(args.eval, Some("SELECT 1".into()));
        assert!(!args.repl);
    }

    #[test]
    fn test_parse_args_format() {
        let args = CliArgs::parse(&["--format".into(), "json".into()]).unwrap();
        assert_eq!(args.format, OutputFormat::Json);
    }

    #[test]
    fn test_print_csv_field() {
        // Just ensure it doesn't panic
        print_csv_field("hello");
        print_csv_field("hello, world");
        print_csv_field("say \"hi\"");
    }
}

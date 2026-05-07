use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::exec::engine::{Engine, OutputFormat, QueryResult};
use crate::storage::{Pager, Wal};
use crate::txn::TransactionManager;
use crate::types::*;
use std::io::{self, BufRead, Write};

#[derive(Debug)]
pub struct CliArgs {
    pub db_path: String,
    pub repl: bool,
    pub eval: Option<String>,
    pub file: Option<String>,
    pub continue_on_error: bool,
    pub format: Option<String>,
    pub quiet: bool,
    pub version: bool,
    pub help: bool,
    pub init: bool,
}

impl CliArgs {
    pub fn parse(args: &[String]) -> std::result::Result<Self, String> {
        let mut db_path = "./minidb.db".to_string();
        let mut repl = false;
        let mut eval = None;
        let mut file = None;
        let mut continue_on_error = false;
        let mut format = None;
        let mut quiet = false;
        let mut version = false;
        let mut help = false;
        let mut init = false;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--db" => {
                    i += 1;
                    db_path = args.get(i).ok_or("--db requires a path argument")?.clone();
                }
                "--repl" => repl = true,
                "--eval" => {
                    i += 1;
                    eval = Some(args.get(i).ok_or("--eval requires a SQL argument")?.clone());
                }
                "--file" => {
                    i += 1;
                    file = Some(
                        args.get(i)
                            .ok_or("--file requires a path argument")?
                            .clone(),
                    );
                }
                "--continue-on-error" => continue_on_error = true,
                "--format" => {
                    i += 1;
                    format = Some(
                        args.get(i)
                            .ok_or("--format requires an argument (table|csv|json)")?
                            .clone(),
                    );
                }
                "--quiet" => quiet = true,
                "--version" => version = true,
                "--help" | "-h" => help = true,
                "--init" => init = true,
                other if other.starts_with('-') => {
                    return Err(format!("unknown option: {}", other));
                }
                _ => {
                    return Err(format!("unexpected argument: {}", args[i]));
                }
            }
            i += 1;
        }

        // Detect TTY for REPL mode
        if !repl && eval.is_none() && file.is_none() {
            if atty_is_tty() {
                repl = true;
            }
        }

        Ok(CliArgs {
            db_path,
            repl,
            eval,
            file,
            continue_on_error,
            format,
            quiet,
            version,
            help,
            init,
        })
    }
}

fn atty_is_tty() -> bool {
    #[cfg(unix)]
    {
        extern "C" {
            fn isatty(fd: i32) -> i32;
        }
        // SAFETY: isatty is a simple libc call with no side effects
        unsafe { isatty(0) != 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

pub fn run(args: &CliArgs) -> i32 {
    if args.version {
        eprintln!("minidb {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }

    if args.help {
        print_help();
        return 0;
    }

    if args.init {
        match Pager::create(&args.db_path) {
            Ok(_) => {
                eprintln!("initialized empty database: {}", args.db_path);
                return 0;
            }
            Err(e) => {
                eprintln!("error: {}", e);
                return 3;
            }
        }
    }

    match run_inner(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {}", e);
            e.exit_code()
        }
    }
}

fn run_inner(args: &CliArgs) -> Result<i32> {
    let wal_path = format!("{}.wal", args.db_path);
    let wal = Wal::open(&wal_path)?;
    let pager = Pager::open(&args.db_path)?;
    let txn = TransactionManager::new(wal);
    let catalog = Catalog::new();

    let mut engine = Engine::new(pager, catalog, txn);

    // Ensure catalog page exists
    if engine.pager.page_count() == 0 {
        engine.pager.allocate_page()?;
    }

    // Recover from WAL
    engine.recover_from_wal()?;
    engine.load_catalog()?;

    // Set output format
    if let Some(fmt) = &args.format {
        match fmt.as_str() {
            "table" => engine.output_format = OutputFormat::Table,
            "csv" => engine.output_format = OutputFormat::Csv,
            "json" => engine.output_format = OutputFormat::Json,
            _ => return Err(MiniDbError::Runtime(format!("unknown format: {}", fmt))),
        }
    }

    if let Some(sql) = &args.eval {
        return execute_sql(&mut engine, sql, args.quiet);
    }

    if let Some(path) = &args.file {
        return execute_file(&mut engine, path, args.continue_on_error, args.quiet);
    }

    if args.repl {
        run_repl(&mut engine, args.quiet)?;
        return Ok(0);
    }

    Ok(0)
}

fn execute_sql(engine: &mut Engine, sql: &str, quiet: bool) -> Result<i32> {
    let statements = crate::lexer::split_statements(sql);
    for sql in &statements {
        match execute_one(engine, sql, quiet) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("error: {}", e);
                return Ok(e.exit_code());
            }
        }
    }
    Ok(0)
}

fn execute_one(engine: &mut Engine, sql: &str, quiet: bool) -> Result<()> {
    let mut parser = crate::parser::Parser::new(sql)?;
    let stmt = parser.parse_statement()?;
    let result = engine.execute(&stmt)?;

    match result {
        QueryResult::Rows { schema, rows } => {
            print_result(engine.output_format, &schema, &rows);
            if !quiet {
                let count = rows.len();
                eprintln!("ok {} row{}", count, if count == 1 { "" } else { "s" });
            }
        }
        QueryResult::RowsAffected(n) => {
            if !quiet {
                eprintln!("ok {} row{} affected", n, if n == 1 { "" } else { "s" });
            }
        }
        QueryResult::Success => {
            if !quiet {
                eprintln!("ok");
            }
        }
    }

    Ok(())
}

fn execute_file(
    engine: &mut Engine,
    path: &str,
    continue_on_error: bool,
    quiet: bool,
) -> Result<i32> {
    let content = std::fs::read_to_string(path).map_err(|e| MiniDbError::Io(e))?;

    let statements = crate::lexer::split_statements(&content);
    for sql in &statements {
        match execute_one(engine, sql, quiet) {
            Ok(_) => {}
            Err(e) => {
                if continue_on_error {
                    eprintln!("error: {}", e);
                } else {
                    eprintln!("error: {}", e);
                    return Ok(e.exit_code());
                }
            }
        }
    }

    Ok(0)
}

fn run_repl(engine: &mut Engine, quiet: bool) -> Result<()> {
    let mut input = String::new();
    let stdin = io::stdin();

    loop {
        eprint!("minidb> ");
        io::stderr().flush().ok();

        input.clear();
        match stdin.lock().read_line(&mut input) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => {
                eprintln!("error reading input: {}", e);
                break;
            }
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Handle multi-line input (accumulate until semicolon)
        let mut full_input = trimmed.to_string();
        while !full_input.trim_end().ends_with(';') {
            eprint!("    ...> ");
            io::stderr().flush().ok();

            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
            full_input.push(' ');
            full_input.push_str(line.trim());
        }

        // Remove trailing semicolon for meta commands
        let sql = full_input.trim();

        // Handle .exit without semicolon
        if sql == ".exit" || sql == ".quit" {
            break;
        }

        let statements = crate::lexer::split_statements(sql);
        for stmt_sql in &statements {
            match execute_one(engine, stmt_sql, quiet) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("error: {}", e);
                }
            }
        }
    }

    Ok(())
}

fn print_result(format: OutputFormat, schema: &Schema, rows: &[Vec<Value>]) {
    match format {
        OutputFormat::Table => print_table(schema, rows),
        OutputFormat::Csv => print_csv(schema, rows),
        OutputFormat::Json => print_json(schema, rows),
    }
}

fn print_table(schema: &Schema, rows: &[Vec<Value>]) {
    let col_names: Vec<&str> = schema.column_names();

    // Calculate column widths
    let mut widths: Vec<usize> = col_names.iter().map(|n| n.len()).collect();
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(format!("{}", val).len());
            }
        }
    }

    // Print header
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (i, name) in col_names.iter().enumerate() {
        if i > 0 {
            write!(out, " | ").ok();
        }
        write!(out, "{:>width$}", name, width = widths[i]).ok();
    }
    writeln!(out).ok();

    // Print separator
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            write!(out, "-+-").ok();
        }
        write!(out, "{}", "-".repeat(*w)).ok();
    }
    writeln!(out).ok();

    // Print rows
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i > 0 {
                write!(out, " | ").ok();
            }
            write!(out, "{:>width$}", val, width = widths[i]).ok();
        }
        writeln!(out).ok();
    }
}

fn print_csv(schema: &Schema, rows: &[Vec<Value>]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Header
    let col_names: Vec<&str> = schema.column_names();
    for (i, name) in col_names.iter().enumerate() {
        if i > 0 {
            write!(out, ",").ok();
        }
        write_csv_field(&mut out, name);
    }
    writeln!(out).ok();

    // Rows
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i > 0 {
                write!(out, ",").ok();
            }
            let s = format!("{}", val);
            write_csv_field(&mut out, &s);
        }
        writeln!(out).ok();
    }
}

fn write_csv_field(out: &mut impl Write, s: &str) {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        write!(out, "\"{}\"", s.replace('"', "\"\"")).ok();
    } else {
        write!(out, "{}", s).ok();
    }
}

fn print_json(schema: &Schema, rows: &[Vec<Value>]) {
    let col_names: Vec<&str> = schema.column_names();
    let json_rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let mut map = serde_json::Map::new();
            for (i, val) in row.iter().enumerate() {
                let name = col_names.get(i).unwrap_or(&"expr").to_string();
                let json_val = match val {
                    Value::Null => serde_json::Value::Null,
                    Value::Int(i) => serde_json::Value::Number((*i).into()),
                    Value::Real(r) => serde_json::Number::from_f64(*r)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null),
                    Value::Text(s) => serde_json::Value::String(s.clone()),
                    Value::Bool(b) => serde_json::Value::Bool(*b),
                };
                map.insert(name, json_val);
            }
            serde_json::Value::Object(map)
        })
        .collect();

    if let Ok(json) = serde_json::to_string(&json_rows) {
        println!("{}", json);
    }
}

fn print_help() {
    eprintln!("minidb - an embeddable SQL-subset relational database");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    minidb [OPTIONS]");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("    --db <PATH>            Database file path (default: ./minidb.db)");
    eprintln!("    --repl                 Start interactive REPL");
    eprintln!("    --eval <SQL>           Execute SQL and exit");
    eprintln!("    --file <PATH>          Execute SQL script from file");
    eprintln!("    --continue-on-error    Continue executing script on error");
    eprintln!("    --format <FMT>         Output format: table, csv, json (default: table)");
    eprintln!("    --quiet                Suppress per-statement summary lines");
    eprintln!("    --version              Print version and exit");
    eprintln!("    --help, -h             Print this help message");
    eprintln!("    --init                 Create a fresh empty database and exit");
    eprintln!();
    eprintln!("META-COMMANDS:");
    eprintln!("    .schema                Show all table schemas");
    eprintln!("    .tables                List all tables");
    eprintln!("    .indexes               List all indexes");
    eprintln!("    .format <kind>         Change output format");
    eprintln!("    .exit, .quit           Exit the REPL");
    eprintln!();
    eprintln!("EXIT CODES:");
    eprintln!("    0  Success");
    eprintln!("    1  SQL error");
    eprintln!("    2  CLI usage error");
    eprintln!("    3  Storage/IO error");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_args() {
        let args = CliArgs::parse(&[
            "minidb".into(),
            "--db".into(),
            "test.db".into(),
            "--eval".into(),
            "SELECT 1".into(),
        ])
        .unwrap();
        assert_eq!(args.db_path, "test.db");
        assert_eq!(args.eval, Some("SELECT 1".into()));
    }

    #[test]
    fn test_parse_args_version() {
        let args = CliArgs::parse(&["minidb".into(), "--version".into()]).unwrap();
        assert!(args.version);
    }

    #[test]
    fn test_parse_args_help() {
        let args = CliArgs::parse(&["minidb".into(), "--help".into()]).unwrap();
        assert!(args.help);
    }

    #[test]
    fn test_parse_args_init() {
        let args = CliArgs::parse(&["minidb".into(), "--init".into()]).unwrap();
        assert!(args.init);
    }
}

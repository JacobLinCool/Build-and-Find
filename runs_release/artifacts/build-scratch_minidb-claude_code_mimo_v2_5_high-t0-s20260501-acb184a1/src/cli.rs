use std::io::{self, BufRead, Write};

use crate::exec::{Engine, ResultSet};
use crate::lexer::tokenize;
use crate::parser::parse;
use crate::types::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

pub struct CliOptions {
    pub db_path: String,
    pub eval: Option<String>,
    pub file: Option<String>,
    pub repl: bool,
    pub format: OutputFormat,
    pub quiet: bool,
    pub continue_on_error: bool,
    pub init: bool,
    pub version: bool,
    pub help: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        CliOptions {
            db_path: "./minidb.db".into(),
            eval: None,
            file: None,
            repl: false,
            format: OutputFormat::Table,
            quiet: false,
            continue_on_error: false,
            init: false,
            version: false,
            help: false,
        }
    }
}

pub fn parse_args() -> CliOptions {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut opts = CliOptions::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                i += 1;
                opts.db_path = args.get(i).cloned().unwrap_or_default();
            }
            "--eval" => {
                i += 1;
                opts.eval = args.get(i).cloned();
            }
            "--file" => {
                i += 1;
                opts.file = args.get(i).cloned();
            }
            "--repl" => opts.repl = true,
            "--format" => {
                i += 1;
                if let Some(f) = args.get(i) {
                    opts.format = match f.as_str() {
                        "csv" => OutputFormat::Csv,
                        "json" => OutputFormat::Json,
                        _ => OutputFormat::Table,
                    };
                }
            }
            "--quiet" | "-q" => opts.quiet = true,
            "--continue-on-error" => opts.continue_on_error = true,
            "--init" => opts.init = true,
            "--version" | "-v" => opts.version = true,
            "--help" | "-h" => opts.help = true,
            _ => {}
        }
        i += 1;
    }
    opts
}

pub fn print_help() {
    eprintln!("minidb - an embeddable SQL-subset relational database");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  minidb [OPTIONS]");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("  --db <PATH>              Database file path (default: ./minidb.db)");
    eprintln!("  --eval \"<SQL>\"           Execute SQL statement(s) and exit");
    eprintln!("  --file <PATH>            Execute SQL script file");
    eprintln!("  --repl                   Force interactive REPL mode");
    eprintln!("  --format <kind>           Output format: table, csv, json (default: table)");
    eprintln!("  --quiet, -q              Suppress per-statement summary lines");
    eprintln!("  --continue-on-error     Continue executing on errors in --file mode");
    eprintln!("  --init                   Create a fresh empty database and exit");
    eprintln!("  --version, -v            Print version");
    eprintln!("  --help, -h               Print this help");
    eprintln!();
    eprintln!("META COMMANDS:");
    eprintln!("  .schema                  Show table schemas");
    eprintln!("  .tables                  List all tables");
    eprintln!("  .indexes                 List all indexes");
    eprintln!("  .format <kind>           Set output format (table, csv, json)");
    eprintln!("  .exit                    Exit the REPL");
}

pub fn print_version() {
    eprintln!("minidb {}", env!("CARGO_PKG_VERSION"));
}

pub fn run() -> i32 {
    let opts = parse_args();

    if opts.version {
        print_version();
        return 0;
    }

    if opts.help {
        print_help();
        return 0;
    }

    // Initialize logger
    let _ = env_logger::try_init();

    if opts.init {
        let path = std::path::Path::new(&opts.db_path);
        match Engine::open(path) {
            Ok(_) => {
                eprintln!("ok database initialized at {}", opts.db_path);
                return 0;
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                return 2;
            }
        }
    }

    let mut engine = match Engine::open(std::path::Path::new(&opts.db_path)) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error opening database: {}", e);
            return 3;
        }
    };

    let mut format = opts.format.clone();

    if let Some(ref sql) = opts.eval {
        let exit_code = execute_sql_string(&mut engine, sql, &format, opts.quiet);
        return exit_code;
    }

    if let Some(ref file_path) = opts.file {
        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error reading file: {}", e);
                return 2;
            }
        };
        return execute_sql_string(&mut engine, &content, &format, opts.quiet);
    }

    // REPL mode
    let is_tty = atty::is(atty::Stream::Stdin);
    if is_tty || opts.repl {
        run_repl(&mut engine, &mut format, opts.quiet);
        return 0;
    }

    // Batch mode: read from stdin
    let stdin = io::stdin();
    let mut buffer = String::new();
    for line in stdin.lock().lines() {
        match line {
            Ok(l) => {
                buffer.push_str(&l);
                buffer.push('\n');
            }
            Err(_) => break,
        }
    }

    let exit_code = execute_sql_string(&mut engine, &buffer, &format, opts.quiet);
    exit_code
}

fn run_repl(engine: &mut Engine, format: &mut OutputFormat, quiet: bool) {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        eprint!("minidb> ");
        stdout.flush().ok();

        let mut buffer = String::new();
        let mut found_semicolon = false;

        loop {
            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(0) => {
                    // EOF
                    eprintln!();
                    return;
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed == ".exit" {
                        return;
                    }
                    if trimmed == ".schema" {
                        print_schema(engine);
                        break;
                    }
                    if trimmed == ".tables" {
                        print_tables(engine);
                        break;
                    }
                    if trimmed == ".indexes" {
                        print_indexes(engine);
                        break;
                    }
                    if trimmed.starts_with(".format ") {
                        let f = trimmed[8..].trim();
                        *format = match f {
                            "csv" => OutputFormat::Csv,
                            "json" => OutputFormat::Json,
                            _ => OutputFormat::Table,
                        };
                        eprintln!(
                            "ok format set to {}",
                            match format {
                                OutputFormat::Table => "table",
                                OutputFormat::Csv => "csv",
                                OutputFormat::Json => "json",
                            }
                        );
                        break;
                    }
                    buffer.push_str(&line);
                    if line.contains(';') {
                        found_semicolon = true;
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("Error reading input: {}", e);
                    return;
                }
            }
        }

        if !found_semicolon && buffer.trim().is_empty() {
            continue;
        }

        let sql = buffer.trim();
        if sql.is_empty() {
            continue;
        }

        let exit_code = execute_sql_string(engine, sql, format, quiet);
        if exit_code != 0 {
            // Error already printed by execute_sql_string
        }
    }
}

fn execute_sql_string(engine: &mut Engine, sql: &str, format: &OutputFormat, quiet: bool) -> i32 {
    // Split by semicolons (carefully handling strings)
    let statements = split_statements(sql);

    for stmt_str in &statements {
        let trimmed = stmt_str.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Handle meta-commands (strip trailing semicolons)
        let meta_cmd = trimmed.trim_end_matches(';').trim();
        if meta_cmd == ".tables" {
            print_tables(engine);
            continue;
        }
        if meta_cmd == ".schema" {
            print_schema(engine);
            continue;
        }
        if meta_cmd == ".indexes" {
            print_indexes(engine);
            continue;
        }

        let tokens = match tokenize(trimmed) {
            Ok(t) => t,
            Err((msg, line, col)) => {
                eprintln!("Error at {}:{}: {}", line, col, msg);
                return 1;
            }
        };

        let stmts = match parse(tokens) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: {}", e);
                return 1;
            }
        };

        for stmt in stmts {
            match engine.execute_statement(stmt) {
                Ok(rs) => {
                    print_result_set(&rs, format);
                    if !quiet {
                        if rs.rows_affected > 0 {
                            eprintln!(
                                "ok {} row{} affected",
                                rs.rows_affected,
                                if rs.rows_affected == 1 { "" } else { "s" }
                            );
                        } else if rs.rows.len() > 0 {
                            eprintln!(
                                "ok {} row{}",
                                rs.rows.len(),
                                if rs.rows.len() == 1 { "" } else { "s" }
                            );
                        } else {
                            eprintln!("ok");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    return 1;
                }
            }
        }
    }

    0
}

fn split_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut string_char = ' ';

    for ch in sql.chars() {
        if in_string {
            current.push(ch);
            if ch == string_char {
                in_string = false;
            }
        } else if ch == '\'' {
            in_string = true;
            string_char = ch;
            current.push(ch);
        } else if ch == ';' {
            current.push(';');
            statements.push(current.clone());
            current.clear();
        } else {
            current.push(ch);
        }
    }

    if !current.trim().is_empty() {
        // Append semicolon if missing
        let trimmed = current.trim();
        if !trimmed.ends_with(';') {
            current.push(';');
        }
        statements.push(current);
    }

    statements
}

fn print_result_set(rs: &ResultSet, format: &OutputFormat) {
    if rs.rows.is_empty() && rs.schema.columns.is_empty() {
        return;
    }

    match format {
        OutputFormat::Table => print_table(rs),
        OutputFormat::Csv => print_csv(rs),
        OutputFormat::Json => print_json(rs),
    }
}

fn print_table(rs: &ResultSet) {
    if rs.rows.is_empty() {
        return;
    }

    let col_count = rs.schema.columns.len();
    if col_count == 0 {
        return;
    }

    let mut widths: Vec<usize> = rs
        .schema
        .columns
        .iter()
        .map(|(name, _)| name.len())
        .collect();

    for row in &rs.rows {
        for (i, val) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(format!("{}", val).len());
            }
        }
    }

    // Header
    for (i, (name, _)) in rs.schema.columns.iter().enumerate() {
        if i > 0 {
            print!(" | ");
        }
        print!("{:<width$}", name, width = widths[i]);
    }
    println!();

    // Separator
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            print!("-+-");
        }
        print!("{}", "-".repeat(*w));
    }
    println!();

    // Rows
    for row in &rs.rows {
        for (i, val) in row.iter().enumerate() {
            if i > 0 {
                print!(" | ");
            }
            let s = format!("{}", val);
            print!("{:<width$}", s, width = widths[i]);
        }
        println!();
    }
}

fn print_csv(rs: &ResultSet) {
    let mut wtr = csv::Writer::from_writer(io::stdout());

    // Header
    let headers: Vec<String> = rs
        .schema
        .columns
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    wtr.write_record(&headers).ok();

    for row in &rs.rows {
        let vals: Vec<String> = row.iter().map(|v| format!("{}", v)).collect();
        wtr.write_record(&vals).ok();
    }

    wtr.flush().ok();
}

fn print_json(rs: &ResultSet) {
    let mut rows = Vec::new();
    for row in &rs.rows {
        let mut map = serde_json::Map::new();
        for (i, (name, _)) in rs.schema.columns.iter().enumerate() {
            let val = if i < row.len() {
                match &row[i] {
                    Value::Null => serde_json::Value::Null,
                    Value::Int(v) => serde_json::Value::Number((*v).into()),
                    Value::Real(v) => serde_json::Number::from_f64(*v)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null),
                    Value::Text(s) => serde_json::Value::String(s.clone()),
                    Value::Bool(b) => serde_json::Value::Bool(*b),
                }
            } else {
                serde_json::Value::Null
            };
            map.insert(name.clone(), val);
        }
        rows.push(serde_json::Value::Object(map));
    }

    println!("{}", serde_json::to_string_pretty(&rows).unwrap());
}

fn print_schema(engine: &Engine) {
    for (name, schema) in engine.catalog.all_tables() {
        eprintln!("CREATE TABLE {} (", name);
        for (i, col) in schema.columns.iter().enumerate() {
            let mut parts = vec![col.name.clone(), col.col_type.to_string()];
            if col.primary_key {
                parts.push("PRIMARY KEY".into());
            }
            if !col.nullable && !col.primary_key {
                parts.push("NOT NULL".into());
            }
            if col.unique {
                parts.push("UNIQUE".into());
            }
            if let Some(ref default) = col.default {
                parts.push(format!("DEFAULT {}", default));
            }
            let suffix = if i < schema.columns.len() - 1 {
                ","
            } else {
                ""
            };
            eprintln!("  {}{}", parts.join(" "), suffix);
        }
        eprintln!(");");
    }
}

fn print_tables(engine: &Engine) {
    for name in engine.catalog.table_names() {
        eprintln!("{}", name);
    }
}

fn print_indexes(engine: &Engine) {
    for (name, idx) in engine.catalog.all_indexes() {
        eprintln!("{} ON {} ({})", name, idx.table, idx.column);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_statements() {
        let sqls = split_statements("SELECT 1; SELECT 2;");
        assert_eq!(sqls.len(), 2);
    }

    #[test]
    fn test_split_preserves_strings() {
        let sqls = split_statements("SELECT 'hello;world'; SELECT 1;");
        assert_eq!(sqls.len(), 2);
        assert!(sqls[0].contains("hello;world"));
    }
}

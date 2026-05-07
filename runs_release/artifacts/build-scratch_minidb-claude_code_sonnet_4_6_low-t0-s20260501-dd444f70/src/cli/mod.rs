use crate::error::{MiniDbError, Result};
use crate::exec::engine::{ExecResult, Executor};
use crate::exec::Schema;
use crate::parser::ast::Stmt;
use crate::parser::parse;
use crate::types::Value;
use std::io::{self, BufRead, Write};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

pub struct CliSession {
    pub executor: Executor,
    pub format: OutputFormat,
    pub quiet: bool,
}

impl CliSession {
    pub fn new(executor: Executor, format: OutputFormat, quiet: bool) -> Self {
        CliSession {
            executor,
            format,
            quiet,
        }
    }

    pub fn run_sql(&mut self, sql: &str) -> Result<Vec<ExecResult>> {
        let stmts = parse(sql)?;
        let mut results = Vec::new();
        for stmt in &stmts {
            if let Stmt::MetaFormat(fmt) = stmt {
                self.format = parse_format(fmt);
                results.push(ExecResult {
                    schema: vec![],
                    rows: vec![],
                    rows_affected: None,
                    summary: "ok".to_string(),
                });
                continue;
            }
            let result = self.executor.execute(stmt)?;
            results.push(result);
        }
        Ok(results)
    }

    pub fn print_result(&self, result: &ExecResult) {
        if !result.rows.is_empty() || !result.schema.is_empty() {
            match self.format {
                OutputFormat::Table => self.print_table(&result.schema, &result.rows),
                OutputFormat::Csv => self.print_csv(&result.schema, &result.rows),
                OutputFormat::Json => self.print_json(&result.schema, &result.rows),
            }
        }
        if !self.quiet {
            eprintln!("{}", result.summary);
        }
    }

    fn print_table(&self, schema: &Schema, rows: &[crate::types::Row]) {
        if schema.is_empty() {
            return;
        }
        // Calculate column widths
        let mut widths: Vec<usize> = schema.iter().map(|e| e.name.len()).collect();
        for row in rows {
            for (i, val) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(val.to_display_string().len());
                }
            }
        }

        // Print header
        let header: Vec<String> = schema
            .iter()
            .enumerate()
            .map(|(i, e)| format!("{:width$}", e.name, width = widths[i]))
            .collect();
        println!("{}", header.join(" | "));

        // Separator
        let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
        println!("{}", sep.join("-+-"));

        // Rows
        for row in rows {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let w = widths.get(i).copied().unwrap_or(0);
                    format!("{:width$}", v.to_display_string(), width = w)
                })
                .collect();
            println!("{}", cells.join(" | "));
        }
    }

    fn print_csv(&self, schema: &Schema, rows: &[crate::types::Row]) {
        // Header
        let header: Vec<String> = schema.iter().map(|e| csv_quote(&e.name)).collect();
        println!("{}", header.join(","));
        // Rows
        for row in rows {
            let cells: Vec<String> = row
                .iter()
                .map(|v| csv_quote(&v.to_display_string()))
                .collect();
            println!("{}", cells.join(","));
        }
    }

    fn print_json(&self, schema: &Schema, rows: &[crate::types::Row]) {
        let json_rows: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                let mut obj = serde_json::Map::new();
                for (i, entry) in schema.iter().enumerate() {
                    let v = row.get(i).unwrap_or(&Value::Null);
                    let jv = value_to_json(v);
                    obj.insert(entry.name.clone(), jv);
                }
                serde_json::Value::Object(obj)
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string(&json_rows).unwrap_or("[]".to_string())
        );
    }
}

fn csv_quote(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Int(n) => serde_json::Value::Number(serde_json::Number::from(*n)),
        Value::Real(r) => serde_json::Number::from_f64(*r)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Text(s) => serde_json::Value::String(s.clone()),
        Value::Bool(b) => serde_json::Value::Bool(*b),
    }
}

fn parse_format(fmt: &str) -> OutputFormat {
    match fmt.to_lowercase().as_str() {
        "csv" => OutputFormat::Csv,
        "json" => OutputFormat::Json,
        _ => OutputFormat::Table,
    }
}

pub fn run_repl(session: &mut CliSession) {
    let stdin = io::stdin();
    let mut buffer = String::new();
    let is_tty = atty_check();

    if is_tty {
        print!("minidb> ");
        let _ = io::stdout().flush();
    }

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim() == ".exit" || line.trim() == ".quit" {
            break;
        }

        buffer.push_str(&line);
        buffer.push('\n');

        // Check for meta commands (single-line, no semicolon needed)
        let trimmed = line.trim();
        if trimmed.starts_with('.') {
            let sql = buffer.drain(..).collect::<String>();
            exec_and_print(session, &sql);
            if is_tty {
                print!("minidb> ");
                let _ = io::stdout().flush();
            }
            continue;
        }

        // Execute on semicolon
        if buffer.trim_end().ends_with(';') {
            let sql = buffer.drain(..).collect::<String>();
            let should_exit = exec_and_print_check_exit(session, &sql);
            if should_exit {
                break;
            }
            if is_tty {
                print!("minidb> ");
                let _ = io::stdout().flush();
            }
        } else if is_tty {
            print!("   ...> ");
            let _ = io::stdout().flush();
        }
    }
}

fn exec_and_print(session: &mut CliSession, sql: &str) {
    exec_and_print_check_exit(session, sql);
}

fn exec_and_print_check_exit(session: &mut CliSession, sql: &str) -> bool {
    match session.run_sql(sql) {
        Ok(results) => {
            for result in &results {
                session.print_result(result);
            }
        }
        Err(MiniDbError::Runtime(ref msg)) if msg == "__exit__" => {
            return true;
        }
        Err(e) => {
            eprintln!("Error: {}", e);
        }
    }
    false
}

pub fn run_eval(session: &mut CliSession, sql: &str) -> i32 {
    match session.run_sql(sql) {
        Ok(results) => {
            for result in &results {
                session.print_result(result);
            }
            0
        }
        Err(MiniDbError::Io(e)) => {
            eprintln!("IO error: {}", e);
            3
        }
        Err(MiniDbError::Storage(e)) => {
            eprintln!("Storage error: {}", e);
            3
        }
        Err(MiniDbError::Wal(e)) => {
            eprintln!("WAL error: {}", e);
            3
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            1
        }
    }
}

pub fn run_file(session: &mut CliSession, path: &str, continue_on_error: bool) -> i32 {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cannot read file '{}': {}", path, e);
            return 3;
        }
    };
    run_eval_multi(session, &content, continue_on_error)
}

fn run_eval_multi(session: &mut CliSession, sql: &str, continue_on_error: bool) -> i32 {
    let stmts = match parse(sql) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Parse error: {}", e);
            return 1;
        }
    };

    let mut exit_code = 0;
    for stmt in &stmts {
        if let Stmt::MetaFormat(fmt) = stmt {
            session.format = parse_format(fmt);
            continue;
        }
        match session.executor.execute(stmt) {
            Ok(result) => {
                session.print_result(&result);
            }
            Err(MiniDbError::Io(e)) => {
                eprintln!("IO error: {}", e);
                exit_code = 3;
                if !continue_on_error {
                    return exit_code;
                }
            }
            Err(MiniDbError::Storage(e)) | Err(MiniDbError::Wal(e)) => {
                eprintln!("Storage error: {}", e);
                exit_code = 3;
                if !continue_on_error {
                    return exit_code;
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                if exit_code == 0 {
                    exit_code = 1;
                }
                if !continue_on_error {
                    return exit_code;
                }
            }
        }
    }
    exit_code
}

fn atty_check() -> bool {
    // Simple TTY check
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe { libc_isatty(0) }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(unix)]
fn libc_isatty(fd: i32) -> bool {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(fd) != 0 }
}

use crate::db::{Database, ExecResult, ResultKind};
use crate::error::{MiniDbError, Result};
use crate::plan::Schema;
use crate::types::Value;
use std::io::{self, Write};

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "table" => Some(OutputFormat::Table),
            "csv" => Some(OutputFormat::Csv),
            "json" => Some(OutputFormat::Json),
            _ => None,
        }
    }
}

pub struct Repl {
    db: Database,
    format: OutputFormat,
    quiet: bool,
}

impl Repl {
    pub fn new(db: Database, format: OutputFormat, quiet: bool) -> Self {
        Repl { db, format, quiet }
    }

    pub fn run_interactive(&mut self) -> Result<()> {
        let mut rl =
            rustyline::DefaultEditor::new().map_err(|e| MiniDbError::Runtime(e.to_string()))?;

        println!(
            "minidb v{} - type SQL statements or .help for meta-commands",
            env!("CARGO_PKG_VERSION")
        );

        let mut buf = String::new();
        loop {
            let prompt = if buf.is_empty() {
                "minidb> "
            } else {
                "   ...> "
            };
            match rl.readline(prompt) {
                Ok(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    // Meta-commands don't need semicolons
                    if trimmed.starts_with('.') {
                        let _ = rl.add_history_entry(trimmed);
                        if let Err(e) = self.execute_line(trimmed) {
                            eprintln!("Error: {}", e);
                        }
                        continue;
                    }

                    buf.push_str(&line);
                    buf.push('\n');

                    // Execute when we see a semicolon at end of trimmed line
                    if trimmed.ends_with(';')
                        || trimmed == "COMMIT"
                        || trimmed == "ROLLBACK"
                        || trimmed.to_uppercase().starts_with("BEGIN")
                        || trimmed.to_uppercase() == "COMMIT"
                        || trimmed.to_uppercase() == "ROLLBACK"
                    {
                        let sql = buf.trim().to_string();
                        buf.clear();
                        let _ = rl.add_history_entry(&sql);
                        if let Err(e) = self.execute_line(&sql) {
                            eprintln!("Error: {}", e);
                        }
                    }
                }
                Err(rustyline::error::ReadlineError::Eof) => {
                    if !buf.trim().is_empty() {
                        let sql = buf.trim().to_string();
                        let _ = self.execute_line(&sql);
                    }
                    break;
                }
                Err(rustyline::error::ReadlineError::Interrupted) => {
                    buf.clear();
                    eprintln!("^C");
                }
                Err(e) => {
                    eprintln!("readline error: {}", e);
                    break;
                }
            }
        }
        Ok(())
    }

    fn execute_line(&mut self, sql: &str) -> Result<()> {
        // Check for .exit
        if sql.trim() == ".exit" || sql.trim() == ".quit" {
            std::process::exit(0);
        }

        // Check for .format command
        if sql.trim().starts_with(".format") {
            let parts: Vec<&str> = sql.trim().splitn(2, ' ').collect();
            if parts.len() == 2 {
                if let Some(fmt) = OutputFormat::from_str(parts[1].trim()) {
                    self.format = fmt;
                    if !self.quiet {
                        eprintln!("ok output format set");
                    }
                } else {
                    eprintln!("Error: unknown format '{}'", parts[1].trim());
                }
            }
            return Ok(());
        }

        match self.db.execute_sql(sql) {
            Ok(results) => {
                for res in results {
                    self.print_result(&res)?;
                }
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub fn print_result(&self, res: &ExecResult) -> Result<()> {
        match res.kind {
            ResultKind::Select => {
                match self.format {
                    OutputFormat::Table => print_table(&res.schema, &res.rows),
                    OutputFormat::Csv => print_csv(&res.schema, &res.rows)?,
                    OutputFormat::Json => print_json(&res.schema, &res.rows)?,
                }
                if !self.quiet {
                    eprintln!(
                        "ok {} row{}",
                        res.rows.len(),
                        if res.rows.len() == 1 { "" } else { "s" }
                    );
                }
            }
            ResultKind::Insert | ResultKind::Update | ResultKind::Delete => {
                if !self.quiet {
                    eprintln!(
                        "ok {} row{} affected",
                        res.rows_affected,
                        if res.rows_affected == 1 { "" } else { "s" }
                    );
                }
            }
            ResultKind::DDL => {
                if !self.quiet {
                    eprintln!("ok");
                }
            }
            ResultKind::Txn => {
                if !self.quiet {
                    eprintln!("ok");
                }
            }
            ResultKind::Empty => {}
        }
        Ok(())
    }

    pub fn execute_batch(
        &mut self,
        sql: &str,
        continue_on_error: bool,
    ) -> std::result::Result<(), (MiniDbError, Option<(usize, usize)>)> {
        // Split on semicolons (simple approach)
        let stmts = match crate::parser::Parser::parse(sql) {
            Ok(s) => s,
            Err(e) => return Err((e, None)),
        };

        for stmt in stmts {
            match self.db.execute_statement(stmt) {
                Ok(res) => {
                    let _ = self.print_result(&res);
                }
                Err(e) => {
                    if !continue_on_error {
                        return Err((e, None));
                    } else {
                        eprintln!("Error (continuing): {}", e);
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn print_table(schema: &Schema, rows: &[Vec<Value>]) {
    let headers: Vec<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
    let ncols = headers.len();
    if ncols == 0 {
        return;
    }

    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(format!("{}", val).len());
            }
        }
    }

    // Header
    let sep: String = widths
        .iter()
        .map(|&w| "-".repeat(w + 2))
        .collect::<Vec<_>>()
        .join("+");
    let sep = format!("+{}+", sep);
    println!("{}", sep);
    let header_row: String = headers
        .iter()
        .zip(widths.iter())
        .map(|(h, &w)| format!(" {:width$} ", h, width = w))
        .collect::<Vec<_>>()
        .join("|");
    println!("|{}|", header_row);
    println!("{}", sep);

    for row in rows {
        let cells: Vec<String> = (0..ncols)
            .map(|i| {
                let val = row.get(i).map(|v| format!("{}", v)).unwrap_or_default();
                format!(" {:width$} ", val, width = widths[i])
            })
            .collect();
        println!("|{}|", cells.join("|"));
    }
    println!("{}", sep);
}

pub fn print_csv(schema: &Schema, rows: &[Vec<Value>]) -> Result<()> {
    let stdout = io::stdout();
    let mut wtr = csv::WriterBuilder::new().from_writer(stdout.lock());

    let headers: Vec<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
    wtr.write_record(&headers)
        .map_err(|e| MiniDbError::Runtime(e.to_string()))?;

    for row in rows {
        let record: Vec<String> = row.iter().map(|v| format!("{}", v)).collect();
        wtr.write_record(&record)
            .map_err(|e| MiniDbError::Runtime(e.to_string()))?;
    }
    wtr.flush()
        .map_err(|e| MiniDbError::Runtime(e.to_string()))?;
    Ok(())
}

pub fn print_json(schema: &Schema, rows: &[Vec<Value>]) -> Result<()> {
    let headers: Vec<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
    let objects: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let mut map = serde_json::Map::new();
            for (i, h) in headers.iter().enumerate() {
                let v = row.get(i).cloned().unwrap_or(Value::Null);
                let jv = value_to_json(&v);
                map.insert(h.to_string(), jv);
            }
            serde_json::Value::Object(map)
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&objects).map_err(|e| MiniDbError::Runtime(e.to_string()))?
    );
    Ok(())
}

fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Int(n) => serde_json::Value::Number((*n).into()),
        Value::Real(f) => serde_json::json!(f),
        Value::Text(s) => serde_json::Value::String(s.clone()),
        Value::Bool(b) => serde_json::Value::Bool(*b),
    }
}

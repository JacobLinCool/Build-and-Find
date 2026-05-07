use crate::error::{MiniDbError, Result};
use crate::exec::executor::{Executor, QueryResult};
use crate::lexer;
use crate::parser;
use crate::types::Value;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

pub struct Cli {
    db_path: PathBuf,
    format: OutputFormat,
    quiet: bool,
    repl: bool,
}

impl Cli {
    pub fn new(db_path: PathBuf, format: OutputFormat, quiet: bool, repl: bool) -> Self {
        Self {
            db_path,
            format,
            quiet,
            repl,
        }
    }

    pub fn run(&self) -> Result<()> {
        let mut executor = if self.db_path.exists() {
            Executor::open(&self.db_path)?
        } else {
            Executor::create(&self.db_path)?
        };

        if self.repl {
            self.run_repl(&mut executor)?;
        }

        Ok(())
    }

    pub fn run_repl(&self, executor: &mut Executor) -> Result<()> {
        let stdin = io::stdin();
        let stdout = io::stdout();
        let stderr = io::stderr();
        let mut reader = stdin.lock();
        let mut out = stdout.lock();
        let mut err = stderr.lock();

        let mut buffer = String::new();
        let mut line = String::new();

        loop {
            if buffer.is_empty() {
                write!(out, "minidb> ")?;
                out.flush()?;
            } else {
                write!(out, "   ...> ")?;
                out.flush()?;
            }

            line.clear();
            let bytes_read = reader.read_line(&mut line)?;
            if bytes_read == 0 {
                // EOF
                writeln!(out)?;
                break;
            }

            buffer.push_str(&line);

            // Check if we have a complete statement (ends with ;)
            if let Some(stmt_end) = buffer.rfind(';') {
                let sql = buffer[..stmt_end].trim().to_string();
                buffer = buffer[stmt_end + 1..].to_string();

                if sql.is_empty() {
                    continue;
                }

                match self.execute_sql(executor, &sql) {
                    Ok(result) => {
                        self.print_result(&mut out, &result)?;
                        if !self.quiet && !result.is_query {
                            writeln!(err, "ok")?;
                        } else if !self.quiet && result.is_query {
                            writeln!(err, "ok {} rows", result.rows.len())?;
                        }
                    }
                    Err(MiniDbError::Runtime(msg)) if msg == "exit" => {
                        return Ok(());
                    }
                    Err(e) => {
                        writeln!(err, "Error: {e}")?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn execute_sql(&self, executor: &mut Executor, sql: &str) -> Result<QueryResult> {
        let tokens = lexer::tokenize(sql)?;
        let stmts = parser::parse(tokens)?;

        let mut last_result = QueryResult {
            columns: vec![],
            rows: vec![],
            affected: 0,
            is_query: false,
        };

        for stmt in stmts {
            last_result = executor.execute(stmt)?;
        }

        Ok(last_result)
    }

    pub fn execute_batch(
        &self,
        executor: &mut Executor,
        sql: &str,
        continue_on_error: bool,
    ) -> Result<()> {
        let stderr = io::stderr();
        let mut err = stderr.lock();
        let stdout = io::stdout();
        let mut out = stdout.lock();

        let tokens = lexer::tokenize(sql)?;
        let stmts = parser::parse(tokens)?;

        for stmt in stmts {
            match executor.execute(stmt) {
                Ok(result) => {
                    self.print_result(&mut out, &result)?;
                    if !self.quiet && !result.is_query {
                        writeln!(err, "ok")?;
                    } else if !self.quiet && result.is_query {
                        writeln!(err, "ok {} rows", result.rows.len())?;
                    }
                }
                Err(e) => {
                    writeln!(err, "Error: {e}")?;
                    if !continue_on_error {
                        return Err(e);
                    }
                }
            }
        }

        Ok(())
    }

    fn print_result(&self, out: &mut impl Write, result: &QueryResult) -> Result<()> {
        if result.rows.is_empty() && result.is_query {
            return Ok(());
        }

        if !result.is_query {
            return Ok(());
        }

        match self.format {
            OutputFormat::Table => self.print_table_rows_only(out, result),
            OutputFormat::Csv => self.print_csv(out, result),
            OutputFormat::Json => self.print_json(out, result),
        }
    }

    fn print_table(&self, out: &mut impl Write, result: &QueryResult) -> Result<()> {
        if result.columns.is_empty() {
            return Ok(());
        }

        // Calculate column widths
        let mut widths: Vec<usize> = result.columns.iter().map(|c| c.len()).collect();
        for row in &result.rows {
            for (i, val) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(format!("{val}").len());
                }
            }
        }

        // Header
        write!(out, "| ")?;
        for (i, col) in result.columns.iter().enumerate() {
            write!(out, "{:width$}", col, width = widths[i])?;
            if i < result.columns.len() - 1 {
                write!(out, " | ")?;
            }
        }
        writeln!(out, " |")?;

        // Separator
        write!(out, "+-")?;
        for (i, w) in widths.iter().enumerate() {
            for _ in 0..*w {
                write!(out, "-")?;
            }
            if i < widths.len() - 1 {
                write!(out, "-+-")?;
            }
        }
        writeln!(out, "-+")?;

        // Rows
        for row in &result.rows {
            write!(out, "| ")?;
            for (i, val) in row.iter().enumerate() {
                let s = format!("{val}");
                let w = widths.get(i).copied().unwrap_or(0);
                write!(out, "{:width$}", s, width = w)?;
                if i < result.columns.len() - 1 {
                    write!(out, " | ")?;
                }
            }
            writeln!(out, " |")?;
        }

        Ok(())
    }

    fn print_table_rows_only(&self, out: &mut impl Write, result: &QueryResult) -> Result<()> {
        if result.columns.is_empty() {
            return Ok(());
        }

        // Calculate column widths
        let widths: Vec<usize> = result
            .columns
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let mut w = 0;
                for row in &result.rows {
                    if i < row.len() {
                        w = w.max(format!("{}", row[i]).len());
                    }
                }
                w
            })
            .collect();

        // Rows only (no header)
        for row in &result.rows {
            write!(out, "| ")?;
            for (i, val) in row.iter().enumerate() {
                let s = format!("{val}");
                let w = widths.get(i).copied().unwrap_or(0);
                write!(out, "{:width$}", s, width = w)?;
                if i < result.columns.len() - 1 {
                    write!(out, " | ")?;
                }
            }
            writeln!(out, " |")?;
        }

        Ok(())
    }

    fn print_csv(&self, out: &mut impl Write, result: &QueryResult) -> Result<()> {
        // Header
        for (i, col) in result.columns.iter().enumerate() {
            if i > 0 {
                write!(out, ",")?;
            }
            write_csv_field(out, col)?;
        }
        writeln!(out)?;

        // Rows
        for row in &result.rows {
            for (i, val) in row.iter().enumerate() {
                if i > 0 {
                    write!(out, ",")?;
                }
                write_csv_field(out, &format!("{val}"))?;
            }
            writeln!(out)?;
        }

        Ok(())
    }

    fn print_json(&self, out: &mut impl Write, result: &QueryResult) -> Result<()> {
        let mut rows = Vec::new();
        for row in &result.rows {
            let mut obj = serde_json::Map::new();
            for (i, col) in result.columns.iter().enumerate() {
                let val = if i < row.len() {
                    value_to_json(&row[i])
                } else {
                    serde_json::Value::Null
                };
                obj.insert(col.clone(), val);
            }
            rows.push(serde_json::Value::Object(obj));
        }
        let json = serde_json::to_string(&rows)?;
        writeln!(out, "{json}")?;
        Ok(())
    }
}

fn write_csv_field(out: &mut impl Write, s: &str) -> Result<()> {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        write!(out, "\"")?;
        for ch in s.chars() {
            if ch == '"' {
                write!(out, "\"\"")?;
            } else {
                write!(out, "{ch}")?;
            }
        }
        write!(out, "\"")?;
    } else {
        write!(out, "{s}")?;
    }
    Ok(())
}

fn value_to_json(val: &Value) -> serde_json::Value {
    match val {
        Value::Null => serde_json::Value::Null,
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Real(r) => serde_json::Number::from_f64(*r)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Text(s) => serde_json::Value::String(s.clone()),
        Value::Bool(b) => serde_json::Value::Bool(*b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_csv_quoting() {
        let mut buf = Vec::new();
        write_csv_field(&mut buf, "hello, world").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "\"hello, world\"");
    }

    #[test]
    fn test_table_output() {
        let mut buf = Vec::new();
        let result = QueryResult {
            columns: vec!["id".to_string(), "name".to_string()],
            rows: vec![
                vec![Value::Int(1), Value::Text("alice".to_string())],
                vec![Value::Int(2), Value::Text("bob".to_string())],
            ],
            affected: 0,
            is_query: true,
        };
        let cli = Cli::new(PathBuf::new(), OutputFormat::Table, false, false);
        cli.print_table(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("alice"));
        assert!(output.contains("bob"));
    }
}

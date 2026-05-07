use crate::catalog::{type_sql, value_sql};
use crate::error::{MiniDbError, Result};
use crate::exec::QueryOutput;
use crate::parser::parse_sql;
use crate::txn::TransactionManager;
use crate::types::Value;
use atty::Stream;
use clap::{Parser, ValueEnum};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "minidb",
    version,
    about = "An embeddable SQL-subset relational database"
)]
struct Args {
    #[arg(long, default_value = "./minidb.db")]
    db: PathBuf,
    #[arg(long)]
    repl: bool,
    #[arg(long)]
    eval: Option<String>,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    continue_on_error: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    format: OutputFormat,
    #[arg(long)]
    init: bool,
    #[arg(long)]
    quiet: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

pub fn run() -> Result<()> {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(err) => {
            let _ = err.print();
            return if err.use_stderr() {
                Err(MiniDbError::Cli(err.to_string()))
            } else {
                Ok(())
            };
        }
    };
    if args.init {
        TransactionManager::init(&args.db)?;
        return Ok(());
    }
    let mut session = Session {
        tm: TransactionManager::open(&args.db)?,
        format: args.format,
        quiet: args.quiet,
    };
    if let Some(sql) = args.eval {
        return session.run_script(&sql, false);
    }
    if let Some(path) = args.file {
        let sql = std::fs::read_to_string(path)?;
        return session.run_script(&sql, args.continue_on_error);
    }
    if args.repl || atty::is(Stream::Stdin) {
        session.repl()
    } else {
        let mut sql = String::new();
        io::stdin().read_to_string(&mut sql)?;
        session.run_script(&sql, args.continue_on_error)
    }
}

struct Session {
    tm: TransactionManager,
    format: OutputFormat,
    quiet: bool,
}

impl Session {
    fn repl(&mut self) -> Result<()> {
        let stdin = io::stdin();
        let mut input = String::new();
        let mut buffer = String::new();
        loop {
            if buffer.trim().is_empty() {
                print!("minidb> ");
            } else {
                print!("   ...> ");
            }
            io::stdout().flush()?;
            input.clear();
            let n = stdin.lock().read_line(&mut input)?;
            if n == 0 {
                return Ok(());
            }
            let trimmed = input.trim();
            if trimmed.starts_with('.') && buffer.trim().is_empty() {
                if self.handle_meta(trimmed)? {
                    return Ok(());
                }
                continue;
            }
            buffer.push_str(&input);
            if statement_complete(&buffer) {
                let script = std::mem::take(&mut buffer);
                self.run_script(&script, false)?;
            }
        }
    }

    fn run_script(&mut self, sql: &str, continue_on_error: bool) -> Result<()> {
        for chunk in split_statements(sql) {
            let text = chunk.trim();
            if text.is_empty() {
                continue;
            }
            let result = if text.starts_with('.') {
                self.handle_meta(text).map(|_| ())
            } else {
                self.run_sql(text)
            };
            if let Err(err) = result {
                if !continue_on_error {
                    return Err(err);
                }
                eprintln!("{err}");
            }
        }
        Ok(())
    }

    fn run_sql(&mut self, sql: &str) -> Result<()> {
        let statements = parse_sql(sql)?;
        for statement in statements {
            let output = self.tm.execute(statement)?;
            self.emit_output(&output)?;
            if !self.quiet {
                self.emit_summary(&output);
            }
        }
        Ok(())
    }

    fn handle_meta(&mut self, command: &str) -> Result<bool> {
        let mut parts = command.split_whitespace();
        let Some(name) = parts.next() else {
            return Ok(false);
        };
        match name {
            ".exit" => Ok(true),
            ".format" => {
                let Some(kind) = parts.next() else {
                    return Err(MiniDbError::Cli("usage: .format table|csv|json".into()));
                };
                self.format = match kind {
                    "table" => OutputFormat::Table,
                    "csv" => OutputFormat::Csv,
                    "json" => OutputFormat::Json,
                    _ => return Err(MiniDbError::Cli("unknown output format".into())),
                };
                Ok(false)
            }
            ".tables" => {
                let columns = vec!["name".to_string()];
                let rows = self
                    .tm
                    .state()
                    .catalog
                    .tables
                    .keys()
                    .map(|name| vec![Value::Text(name.clone())])
                    .collect();
                self.emit_output(&QueryOutput {
                    columns,
                    rows,
                    affected: None,
                    mutation: false,
                })?;
                Ok(false)
            }
            ".indexes" => {
                let columns = vec!["name".into(), "table".into(), "column".into()];
                let rows = self
                    .tm
                    .state()
                    .catalog
                    .indexes
                    .values()
                    .map(|idx| {
                        vec![
                            Value::Text(idx.name.clone()),
                            Value::Text(idx.table.clone()),
                            Value::Text(idx.column.clone()),
                        ]
                    })
                    .collect();
                self.emit_output(&QueryOutput {
                    columns,
                    rows,
                    affected: None,
                    mutation: false,
                })?;
                Ok(false)
            }
            ".schema" => {
                let mut rows = Vec::new();
                for table in self.tm.state().catalog.tables.values() {
                    let cols = table
                        .columns
                        .iter()
                        .map(|c| {
                            let mut s = format!("{} {}", c.name, type_sql(c.ty));
                            if c.primary_key {
                                s.push_str(" PRIMARY KEY");
                            } else {
                                if c.not_null {
                                    s.push_str(" NOT NULL");
                                }
                                if c.unique {
                                    s.push_str(" UNIQUE");
                                }
                            }
                            if !c.default.is_null() {
                                s.push_str(&format!(" DEFAULT {}", value_sql(&c.default)));
                            }
                            s
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    rows.push(vec![Value::Text(format!(
                        "CREATE TABLE {} ({cols});",
                        table.name
                    ))]);
                }
                self.emit_output(&QueryOutput {
                    columns: vec!["sql".into()],
                    rows,
                    affected: None,
                    mutation: false,
                })?;
                Ok(false)
            }
            _ => Err(MiniDbError::Cli(format!("unknown meta-command {name}"))),
        }
    }

    fn emit_output(&self, output: &QueryOutput) -> Result<()> {
        if output.columns.is_empty() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => render_table(output),
            OutputFormat::Csv => render_csv(output),
            OutputFormat::Json => render_json(output),
        }
    }

    fn emit_summary(&self, output: &QueryOutput) {
        if let Some(affected) = output.affected {
            if !output.mutation {
                eprintln!("ok");
            } else if affected == 1 {
                eprintln!("ok 1 row affected");
            } else {
                eprintln!("ok {affected} rows affected");
            }
        } else {
            let n = output.rows.len();
            if n == 1 {
                eprintln!("ok 1 row");
            } else {
                eprintln!("ok {n} rows");
            }
        }
    }
}

fn render_table(output: &QueryOutput) -> Result<()> {
    let mut widths = output.columns.iter().map(String::len).collect::<Vec<_>>();
    for row in &output.rows {
        for (idx, value) in row.iter().enumerate() {
            if let Some(width) = widths.get_mut(idx) {
                *width = (*width).max(value.display().len());
            }
        }
    }
    let mut line = String::new();
    for (idx, col) in output.columns.iter().enumerate() {
        if idx > 0 {
            line.push_str(" | ");
        }
        line.push_str(&pad(col, widths[idx]));
    }
    println!("{line}");
    let sep = widths
        .iter()
        .map(|w| "-".repeat(*w))
        .collect::<Vec<_>>()
        .join("-+-");
    println!("{sep}");
    for row in &output.rows {
        let mut line = String::new();
        for (idx, value) in row.iter().enumerate() {
            if idx > 0 {
                line.push_str(" | ");
            }
            line.push_str(&pad(&value.display(), widths[idx]));
        }
        println!("{line}");
    }
    Ok(())
}

fn render_csv(output: &QueryOutput) -> Result<()> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer
        .write_record(output.columns.iter())
        .map_err(|e| MiniDbError::Storage(format!("write CSV: {e}")))?;
    for row in &output.rows {
        let fields = row.iter().map(Value::display).collect::<Vec<_>>();
        writer
            .write_record(fields.iter())
            .map_err(|e| MiniDbError::Storage(format!("write CSV: {e}")))?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|e| MiniDbError::Storage(format!("finish CSV: {e}")))?;
    let text =
        String::from_utf8(bytes).map_err(|e| MiniDbError::Storage(format!("CSV UTF-8: {e}")))?;
    print!("{text}");
    Ok(())
}

fn render_json(output: &QueryOutput) -> Result<()> {
    let mut rows = Vec::new();
    for row in &output.rows {
        let mut obj = serde_json::Map::new();
        for (idx, value) in row.iter().enumerate() {
            let key = match output.columns.get(idx) {
                Some(column) => column.clone(),
                None => format!("col{idx}"),
            };
            obj.insert(key, json_value(value));
        }
        rows.push(serde_json::Value::Object(obj));
    }
    let text = serde_json::to_string(&rows)
        .map_err(|e| MiniDbError::Storage(format!("encode JSON output: {e}")))?;
    println!("{text}");
    Ok(())
}

fn json_value(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Int(v) => serde_json::Value::Number((*v).into()),
        Value::Real(v) => match serde_json::Number::from_f64(*v) {
            Some(number) => serde_json::Value::Number(number),
            None => serde_json::Value::Null,
        },
        Value::Text(v) => serde_json::Value::String(v.clone()),
        Value::Bool(v) => serde_json::Value::Bool(*v),
    }
}

fn pad(s: &str, width: usize) -> String {
    format!("{s:<width$}")
}

fn statement_complete(sql: &str) -> bool {
    split_statements(sql)
        .last()
        .is_some_and(|s| s.trim_end().ends_with(';'))
        || sql.trim_end().ends_with(';')
}

fn split_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        current.push(ch);
        if ch == '\'' {
            if in_string && chars.peek() == Some(&'\'') {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            } else {
                in_string = !in_string;
            }
        } else if ch == ';' && !in_string {
            out.push(std::mem::take(&mut current));
        } else if ch == '\n' && !in_string && current.trim_start().starts_with('.') {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

trait ReadToString {
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize>;
}

impl ReadToString for io::Stdin {
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        std::io::Read::read_to_string(self, buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitter_ignores_semicolons_inside_strings() {
        let parts = split_statements("select 'a;b'; select 1;");
        assert_eq!(parts.len(), 2);
    }
}

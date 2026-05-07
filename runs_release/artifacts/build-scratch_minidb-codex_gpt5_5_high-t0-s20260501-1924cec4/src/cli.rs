use crate::error::{MiniDbError, Result};
use crate::exec::{Engine, QueryResult};
use crate::parser::ast::{MetaCommand, Statement};
use crate::parser::parse_sql;
use crate::storage::Database;
use crate::types::Value;
use clap::{Parser, ValueEnum};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

#[derive(Debug, Parser)]
#[command(
    name = "minidb",
    version,
    about = "An embeddable SQL-subset relational database"
)]
pub struct Args {
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

pub fn run() -> Result<()> {
    let _ = env_logger::try_init();
    let args = Args::parse();
    if args.init {
        Database::init(&args.db)?;
        return Ok(());
    }
    if args.eval.is_some() && args.file.is_some() {
        return Err(MiniDbError::Usage(
            "--eval and --file may not be used together".into(),
        ));
    }
    let mut session = Session {
        engine: Engine::open(&args.db)?,
        format: args.format,
        quiet: args.quiet,
    };
    if let Some(sql) = args.eval {
        session.run_sql(&sql, false)?;
    } else if let Some(path) = args.file {
        let sql = std::fs::read_to_string(path)?;
        session.run_sql(&sql, args.continue_on_error)?;
    } else if args.repl || io::stdin().is_terminal() {
        session.repl()?;
    } else {
        let sql = io::read_to_string(io::stdin())?;
        session.run_sql(&sql, args.continue_on_error)?;
    }
    Ok(())
}

struct Session {
    engine: Engine,
    format: OutputFormat,
    quiet: bool,
}

impl Session {
    fn run_sql(&mut self, sql: &str, continue_on_error: bool) -> Result<()> {
        let statements = parse_sql(sql)?;
        for stmt in statements {
            match self.run_statement(stmt) {
                Ok(true) => {}
                Ok(false) => break,
                Err(err) if continue_on_error => {
                    eprintln!("{err}");
                }
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    fn run_statement(&mut self, stmt: Statement) -> Result<bool> {
        match stmt {
            Statement::Meta(meta) => self.run_meta(meta),
            stmt => {
                let result = self.engine.execute(stmt)?;
                print_result(&result, self.format)?;
                self.print_summary(&result);
                Ok(true)
            }
        }
    }

    fn run_meta(&mut self, meta: MetaCommand) -> Result<bool> {
        match meta {
            MetaCommand::Schema => {
                let rows = self
                    .engine
                    .db
                    .state
                    .catalog
                    .tables
                    .values()
                    .map(|table| {
                        let cols = table
                            .columns
                            .iter()
                            .map(|c| format!("{} {:?}", c.name, c.data_type))
                            .collect::<Vec<_>>()
                            .join(", ");
                        vec![Value::Text(format!("CREATE TABLE {} ({cols})", table.name))]
                    })
                    .collect::<Vec<_>>();
                let result = QueryResult {
                    columns: vec!["schema".into()],
                    rows,
                    affected: None,
                };
                print_result(&result, self.format)?;
                self.print_summary(&result);
            }
            MetaCommand::Tables => {
                let rows = self
                    .engine
                    .db
                    .state
                    .catalog
                    .tables
                    .values()
                    .map(|t| vec![Value::Text(t.name.clone())])
                    .collect::<Vec<_>>();
                let result = QueryResult {
                    columns: vec!["table".into()],
                    rows,
                    affected: None,
                };
                print_result(&result, self.format)?;
                self.print_summary(&result);
            }
            MetaCommand::Indexes => {
                let rows = self
                    .engine
                    .db
                    .state
                    .catalog
                    .indexes
                    .values()
                    .map(|i| {
                        vec![
                            Value::Text(i.name.clone()),
                            Value::Text(i.table.clone()),
                            Value::Text(i.column.clone()),
                        ]
                    })
                    .collect::<Vec<_>>();
                let result = QueryResult {
                    columns: vec!["index".into(), "table".into(), "column".into()],
                    rows,
                    affected: None,
                };
                print_result(&result, self.format)?;
                self.print_summary(&result);
            }
            MetaCommand::Format(kind) => {
                self.format = match kind.as_str() {
                    "table" => OutputFormat::Table,
                    "csv" => OutputFormat::Csv,
                    "json" => OutputFormat::Json,
                    _ => return Err(MiniDbError::Usage(format!("unknown output format {kind}"))),
                };
                self.print_summary(&QueryResult::ok());
            }
            MetaCommand::Exit => return Ok(false),
        }
        Ok(true)
    }

    fn repl(&mut self) -> Result<()> {
        let stdin = io::stdin();
        let mut buffer = String::new();
        loop {
            let prompt = if buffer.is_empty() {
                "minidb> "
            } else {
                "   ...> "
            };
            print!("{prompt}");
            io::stdout().flush()?;
            let mut line = String::new();
            let bytes = stdin.read_line(&mut line)?;
            if bytes == 0 {
                break;
            }
            if buffer.is_empty() && line.trim_start().starts_with('.') {
                match parse_sql(&line).and_then(|mut s| {
                    s.pop()
                        .ok_or_else(|| MiniDbError::Sql("empty meta-command".into()))
                }) {
                    Ok(stmt) => {
                        if !self.run_statement(stmt)? {
                            break;
                        }
                    }
                    Err(err) => eprintln!("{err}"),
                }
                continue;
            }
            buffer.push_str(&line);
            if line.contains(';') {
                let sql = std::mem::take(&mut buffer);
                if let Err(err) = self.run_sql(&sql, false) {
                    eprintln!("{err}");
                }
            }
        }
        Ok(())
    }

    fn print_summary(&self, result: &QueryResult) {
        if self.quiet {
            return;
        }
        if let Some(n) = result.affected {
            if n == 1 {
                eprintln!("ok 1 row affected");
            } else {
                eprintln!("ok {n} rows affected");
            }
        } else if result.rows.len() == 1 {
            eprintln!("ok 1 row");
        } else {
            eprintln!("ok {} rows", result.rows.len());
        }
    }
}

fn print_result(result: &QueryResult, format: OutputFormat) -> Result<()> {
    if result.columns.is_empty() && result.rows.is_empty() {
        return Ok(());
    }
    match format {
        OutputFormat::Table => print_table(result),
        OutputFormat::Csv => print_csv(result),
        OutputFormat::Json => print_json(result),
    }
}

fn print_table(result: &QueryResult) -> Result<()> {
    if result.columns.is_empty() {
        return Ok(());
    }
    let mut widths = result.columns.iter().map(|c| c.len()).collect::<Vec<_>>();
    for row in &result.rows {
        for (idx, value) in row.iter().enumerate() {
            if let Some(width) = widths.get_mut(idx) {
                *width = (*width).max(value.to_string().len());
            }
        }
    }
    println!("{}", render_table_line(&widths, '='));
    println!("{}", render_table_row(&result.columns, &widths));
    println!("{}", render_table_line(&widths, '-'));
    for row in &result.rows {
        let cells = row.iter().map(ToString::to_string).collect::<Vec<_>>();
        println!("{}", render_table_row(&cells, &widths));
    }
    println!("{}", render_table_line(&widths, '='));
    Ok(())
}

fn render_table_line(widths: &[usize], ch: char) -> String {
    let mut out = String::new();
    out.push('+');
    for width in widths {
        out.push_str(&ch.to_string().repeat(*width + 2));
        out.push('+');
    }
    out
}

fn render_table_row(cells: &[String], widths: &[usize]) -> String {
    let mut out = String::new();
    out.push('|');
    for (idx, width) in widths.iter().enumerate() {
        let cell = cells.get(idx).map(String::as_str).unwrap_or("");
        out.push(' ');
        out.push_str(cell);
        out.push_str(&" ".repeat(width.saturating_sub(cell.len()) + 1));
        out.push('|');
    }
    out
}

fn print_csv(result: &QueryResult) -> Result<()> {
    println!(
        "{}",
        result
            .columns
            .iter()
            .map(|c| csv_escape(c))
            .collect::<Vec<_>>()
            .join(",")
    );
    for row in &result.rows {
        println!(
            "{}",
            row.iter()
                .map(|v| csv_escape(&v.to_string()))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    Ok(())
}

fn csv_escape(input: &str) -> String {
    if input.contains(',') || input.contains('"') || input.contains('\n') || input.contains('\r') {
        format!("\"{}\"", input.replace('"', "\"\""))
    } else {
        input.to_string()
    }
}

fn print_json(result: &QueryResult) -> Result<()> {
    let rows = result
        .rows
        .iter()
        .map(|row| {
            let mut object = serde_json::Map::new();
            for (idx, column) in result.columns.iter().enumerate() {
                object.insert(
                    column.clone(),
                    value_to_json(row.get(idx).unwrap_or(&Value::Null)),
                );
            }
            serde_json::Value::Object(object)
        })
        .collect::<Vec<_>>();
    println!("{}", serde_json::to_string(&rows)?);
    Ok(())
}

fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Int(v) => serde_json::Value::Number((*v).into()),
        Value::Real(v) => serde_json::Number::from_f64(*v)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Text(v) => serde_json::Value::String(v.clone()),
        Value::Bool(v) => serde_json::Value::Bool(*v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_commas_and_quotes() {
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("a\"b"), "\"a\"\"b\"");
    }
}

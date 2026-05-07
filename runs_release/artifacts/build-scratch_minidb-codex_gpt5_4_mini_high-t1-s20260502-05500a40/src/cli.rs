use crate::error::{MiniDbError, Result};
use crate::exec::{summarize_result, ExecutionResult, ResultSet};
use crate::parser::parse_sql;
use crate::txn::Database;
use clap::{ArgAction, Parser, ValueEnum};
use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Table
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "minidb",
    version,
    about = "Embeddable SQL-subset relational database"
)]
struct Cli {
    #[arg(long, default_value = "./minidb.db")]
    db: PathBuf,
    #[arg(long)]
    repl: bool,
    #[arg(long)]
    eval: Option<String>,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long, action = ArgAction::SetTrue)]
    continue_on_error: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    format: OutputFormat,
    #[arg(long, action = ArgAction::SetTrue)]
    quiet: bool,
    #[arg(long, action = ArgAction::SetTrue)]
    init: bool,
}

pub fn run() -> Result<()> {
    match Cli::try_parse() {
        Ok(cli) => run_cli(cli),
        Err(err) => match err.kind() {
            clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
                print!("{err}");
                Ok(())
            }
            _ => Err(MiniDbError::usage(err.to_string())),
        },
    }
}

fn run_cli(cli: Cli) -> Result<()> {
    if cli.init {
        Database::init(&cli.db)?;
        return Ok(());
    }

    let mut session = Session {
        db: Database::open(&cli.db)?,
        format: cli.format,
        quiet: cli.quiet,
    };

    if cli.repl || io::stdin().is_terminal() {
        run_repl(&mut session)
    } else if let Some(sql) = cli.eval {
        run_script(&mut session, &sql, false)
    } else if let Some(path) = cli.file {
        let script = fs::read_to_string(&path)?;
        run_script(&mut session, &script, cli.continue_on_error)
    } else {
        let mut script = String::new();
        io::stdin().read_to_string(&mut script)?;
        run_script(&mut session, &script, false)
    }
}

struct Session {
    db: Database,
    format: OutputFormat,
    quiet: bool,
}

fn run_repl(session: &mut Session) -> Result<()> {
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut input = String::new();
    let mut buffer = String::new();
    loop {
        if stdin.is_terminal() {
            let mut stdout = io::stdout();
            write!(stdout, "minidb> ")?;
            stdout.flush()?;
        }
        input.clear();
        let n = handle.read_line(&mut input)?;
        if n == 0 {
            if !buffer.trim().is_empty() {
                run_script(session, &(buffer.clone() + ";"), false)?;
            }
            return Ok(());
        }
        if buffer.trim().is_empty() && input.trim_start().starts_with('.') {
            if handle_meta(session, input.trim())? {
                return Ok(());
            }
            continue;
        }
        buffer.push_str(&input);
        let (statements, remainder) = split_complete_statements(&buffer);
        buffer = remainder;
        for stmt in statements {
            if stmt.trim().is_empty() {
                continue;
            }
            if stmt.trim_start().starts_with('.') {
                match handle_meta(session, stmt.trim()) {
                    Ok(true) => return Ok(()),
                    Ok(false) => {}
                    Err(err) => eprintln!("{err}"),
                }
            } else {
                if let Err(err) = execute_sql_chunk(session, &stmt) {
                    eprintln!("{err}");
                }
            }
        }
    }
}

fn run_script(session: &mut Session, script: &str, continue_on_error: bool) -> Result<()> {
    let (mut statements, remainder) = split_complete_statements(script);
    if !remainder.trim().is_empty() {
        statements.push(remainder);
    }
    let mut saw_error = false;
    for stmt in statements {
        if stmt.trim().is_empty() {
            continue;
        }
        if stmt.trim_start().starts_with('.') {
            if handle_meta(session, stmt.trim())? {
                return Ok(());
            }
            continue;
        }
        if let Err(err) = execute_sql_chunk(session, &stmt) {
            saw_error = true;
            eprintln!("{err}");
            if !continue_on_error {
                return Err(err);
            }
        }
    }
    if saw_error && !continue_on_error {
        Err(MiniDbError::runtime("script failed"))
    } else {
        Ok(())
    }
}

fn execute_sql_chunk(session: &mut Session, sql: &str) -> Result<()> {
    let statements = parse_sql(sql)?;
    for statement in statements {
        let result = session.db.execute_statement(statement)?;
        render_result(&result, session.format)?;
        if !session.quiet {
            if let Some(summary) = summarize_result(&result) {
                eprintln!("{summary}");
            }
        }
    }
    Ok(())
}

fn render_result(result: &ExecutionResult, format: OutputFormat) -> Result<()> {
    match result {
        ExecutionResult::Query(result) => render_result_set(result, format),
        ExecutionResult::AffectedRows(_) | ExecutionResult::Message(_) | ExecutionResult::Empty => {
            Ok(())
        }
    }
}

fn render_result_set(result: &ResultSet, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Table => {
            print_table(result);
            Ok(())
        }
        OutputFormat::Csv => {
            print_csv(result);
            Ok(())
        }
        OutputFormat::Json => {
            print_json(result)?;
            Ok(())
        }
    }
}

fn print_table(result: &ResultSet) {
    let mut widths = result.columns.iter().map(|c| c.len()).collect::<Vec<_>>();
    for row in &result.rows {
        for (idx, value) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(value.to_string().len());
        }
    }
    let header = result
        .columns
        .iter()
        .enumerate()
        .map(|(idx, col)| format!("{col:<width$}", width = widths[idx]))
        .collect::<Vec<_>>()
        .join(" | ");
    println!("{header}");
    if !result.rows.is_empty() {
        let separator = widths
            .iter()
            .map(|width| "-".repeat(*width))
            .collect::<Vec<_>>()
            .join("-+-");
        println!("{separator}");
        for row in &result.rows {
            let line = row
                .iter()
                .enumerate()
                .map(|(idx, value)| format!("{value:<width$}", width = widths[idx]))
                .collect::<Vec<_>>()
                .join(" | ");
            println!("{line}");
        }
    }
}

fn print_csv(result: &ResultSet) {
    println!(
        "{}",
        result
            .columns
            .iter()
            .map(|col| escape_csv(col))
            .collect::<Vec<_>>()
            .join(",")
    );
    for row in &result.rows {
        println!(
            "{}",
            row.iter()
                .map(|value| escape_csv(&value.to_string()))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
}

fn print_json(result: &ResultSet) -> Result<()> {
    let mut rows = Vec::new();
    for row in &result.rows {
        let mut map = serde_json::Map::new();
        for (idx, value) in row.iter().enumerate() {
            map.insert(result.columns[idx].clone(), json_value(value));
        }
        rows.push(serde_json::Value::Object(map));
    }
    println!(
        "{}",
        serde_json::to_string(&rows).map_err(|err| MiniDbError::runtime(err.to_string()))?
    );
    Ok(())
}

fn json_value(value: &crate::types::Value) -> serde_json::Value {
    match value {
        crate::types::Value::Null => serde_json::Value::Null,
        crate::types::Value::Int(v) => serde_json::Value::from(*v),
        crate::types::Value::Real(v) => serde_json::Value::from(*v),
        crate::types::Value::Text(v) => serde_json::Value::from(v.clone()),
        crate::types::Value::Bool(v) => serde_json::Value::from(*v),
    }
}

fn escape_csv(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn handle_meta(session: &mut Session, command: &str) -> Result<bool> {
    let mut parts = command.split_whitespace();
    let head = parts.next().unwrap_or("");
    match head {
        ".exit" => Ok(true),
        ".format" => {
            let kind = parts
                .next()
                .ok_or_else(|| MiniDbError::usage(".format requires table, csv, or json"))?;
            session.format = match kind.to_ascii_lowercase().as_str() {
                "table" => OutputFormat::Table,
                "csv" => OutputFormat::Csv,
                "json" => OutputFormat::Json,
                _ => return Err(MiniDbError::usage("unknown output format")),
            };
            if !session.quiet {
                eprintln!("ok");
            }
            Ok(false)
        }
        ".tables" => {
            let mut names: Vec<_> = session.db.state().catalog.tables.keys().cloned().collect();
            names.sort();
            println!("{}", names.join("\n"));
            if !session.quiet {
                eprintln!("ok");
            }
            Ok(false)
        }
        ".indexes" => {
            let mut names: Vec<_> = session.db.state().catalog.indexes.keys().cloned().collect();
            names.sort();
            println!("{}", names.join("\n"));
            if !session.quiet {
                eprintln!("ok");
            }
            Ok(false)
        }
        ".schema" => {
            print_schema(session);
            if !session.quiet {
                eprintln!("ok");
            }
            Ok(false)
        }
        _ => Err(MiniDbError::usage(format!("unknown meta-command {head}"))),
    }
}

fn print_schema(session: &Session) {
    for table in session.db.state().catalog.tables.values() {
        let cols = table
            .columns
            .iter()
            .map(|column| {
                let mut parts = vec![
                    column.name.clone(),
                    format!("{:?}", column.col_type).to_uppercase(),
                ];
                if column.primary_key {
                    parts.push("PRIMARY KEY".into());
                }
                if column.not_null {
                    parts.push("NOT NULL".into());
                }
                if column.unique {
                    parts.push("UNIQUE".into());
                }
                if let Some(default) = &column.default {
                    parts.push(format!("DEFAULT {}", default));
                }
                parts.join(" ")
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("CREATE TABLE {} ({});", table.name, cols);
    }
    for index in session.db.state().catalog.indexes.values() {
        println!(
            "CREATE INDEX {} ON {} ({});",
            index.name, index.table, index.column
        );
    }
}

fn split_complete_statements(input: &str) -> (Vec<String>, String) {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        current.push(ch);
        match ch {
            '\'' => {
                if in_string && chars.peek() == Some(&'\'') {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                } else {
                    in_string = !in_string;
                }
            }
            ';' if !in_string => {
                statements.push(current.trim().to_string());
                current.clear();
            }
            _ => {}
        }
    }
    (statements, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_statements() {
        let (stmts, rest) = split_complete_statements("select 1; select 2");
        assert_eq!(stmts.len(), 1);
        assert_eq!(rest.trim(), "select 2");
    }
}

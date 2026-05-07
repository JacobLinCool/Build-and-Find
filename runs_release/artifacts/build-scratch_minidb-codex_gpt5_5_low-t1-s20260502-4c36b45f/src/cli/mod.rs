use clap::{Parser, ValueEnum};
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::PathBuf,
};

use crate::{
    error::{MiniDbError, Result},
    exec::QueryResult,
    parser::{
        ast::{MetaCommand, Statement},
        parse_script,
    },
    storage::Storage,
    txn::TransactionManager,
    types::Value,
};

#[derive(Parser)]
#[command(version, about = "minidb embeddable SQL-subset database")]
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
    #[arg(long, value_enum, default_value = "table")]
    format: OutputFormat,
    #[arg(long)]
    init: bool,
    #[arg(long)]
    quiet: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

pub fn run() -> i32 {
    match run_inner() {
        Ok(code) => code,
        Err(MiniDbError::Io(e)) => {
            eprintln!("storage/IO error: {e}");
            3
        }
        Err(MiniDbError::Storage(e)) => {
            eprintln!("storage/IO error: {e}");
            3
        }
        Err(MiniDbError::Usage(e)) => {
            eprintln!("{e}");
            2
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn run_inner() -> Result<i32> {
    let args = Args::try_parse().map_err(|e| MiniDbError::Usage(e.to_string()))?;
    if args.init {
        Storage::init(&args.db)?;
        return Ok(0);
    }
    let mut storage = Storage::open(&args.db)?;
    let mut tm = TransactionManager::default();
    let mut session = Session {
        format: args.format,
        quiet: args.quiet,
    };
    if let Some(sql) = args.eval {
        return execute_script(&sql, &mut storage, &mut tm, &mut session, false);
    }
    if let Some(path) = args.file {
        let sql = fs::read_to_string(path)?;
        return execute_script(
            &sql,
            &mut storage,
            &mut tm,
            &mut session,
            args.continue_on_error,
        );
    }
    if args.repl || io::stdin().is_terminal() {
        repl(&mut storage, &mut tm, &mut session)
    } else {
        let sql = io::read_to_string(io::stdin())?;
        execute_script(
            &sql,
            &mut storage,
            &mut tm,
            &mut session,
            args.continue_on_error,
        )
    }
}

struct Session {
    format: OutputFormat,
    quiet: bool,
}

fn execute_script(
    sql: &str,
    storage: &mut Storage,
    tm: &mut TransactionManager,
    session: &mut Session,
    cont: bool,
) -> Result<i32> {
    let stmts = parse_script(sql)?;
    for stmt in stmts {
        match execute_one(stmt, storage, tm, session) {
            Ok(exit) if exit => return Ok(0),
            Ok(_) => {}
            Err(e) if cont => eprintln!("{e}"),
            Err(e) => return Err(e),
        }
    }
    Ok(0)
}

fn repl(storage: &mut Storage, tm: &mut TransactionManager, session: &mut Session) -> Result<i32> {
    let mut buf = String::new();
    loop {
        print!("minidb> ");
        io::stdout().flush()?;
        buf.clear();
        let n = io::stdin().read_line(&mut buf)?;
        if n == 0 {
            return Ok(0);
        }
        if !buf.trim_start().starts_with('.') && !buf.trim_end().ends_with(';') {
            loop {
                print!("   ...> ");
                io::stdout().flush()?;
                let mut line = String::new();
                let n = io::stdin().read_line(&mut line)?;
                if n == 0 {
                    break;
                }
                buf.push_str(&line);
                if line.trim_end().ends_with(';') {
                    break;
                }
            }
        }
        match execute_script(&buf, storage, tm, session, false) {
            Ok(_) => {}
            Err(e) => eprintln!("{e}"),
        }
    }
}

fn execute_one(
    stmt: Statement,
    storage: &mut Storage,
    tm: &mut TransactionManager,
    session: &mut Session,
) -> Result<bool> {
    match stmt {
        Statement::Meta(MetaCommand::Exit) => Ok(true),
        Statement::Meta(MetaCommand::Format(kind)) => {
            session.format = match kind.as_str() {
                "table" => OutputFormat::Table,
                "csv" => OutputFormat::Csv,
                "json" => OutputFormat::Json,
                _ => return Err(MiniDbError::Sql(format!("unknown format '{kind}'"))),
            };
            Ok(false)
        }
        Statement::Meta(MetaCommand::Tables) => {
            let rows = storage
                .image
                .catalog
                .tables
                .keys()
                .map(|t| vec![Value::Text(t.clone())])
                .collect();
            emit(
                QueryResult {
                    columns: vec!["table".into()],
                    rows,
                    affected: None,
                    message: "ok".into(),
                },
                session,
            )?;
            Ok(false)
        }
        Statement::Meta(MetaCommand::Indexes) => {
            let rows = storage
                .image
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
                .collect();
            emit(
                QueryResult {
                    columns: vec!["index".into(), "table".into(), "column".into()],
                    rows,
                    affected: None,
                    message: "ok".into(),
                },
                session,
            )?;
            Ok(false)
        }
        Statement::Meta(MetaCommand::Schema) => {
            let rows = storage
                .image
                .catalog
                .tables
                .values()
                .map(|t| {
                    let cols = t
                        .columns
                        .iter()
                        .map(|c| format!("{} {:?}", c.name, c.ty))
                        .collect::<Vec<_>>()
                        .join(", ");
                    vec![Value::Text(format!("CREATE TABLE {} ({cols})", t.name))]
                })
                .collect();
            emit(
                QueryResult {
                    columns: vec!["schema".into()],
                    rows,
                    affected: None,
                    message: "ok".into(),
                },
                session,
            )?;
            Ok(false)
        }
        other => {
            let q = tm.execute(storage, &other)?;
            emit(q, session)?;
            Ok(false)
        }
    }
}

fn emit(result: QueryResult, session: &Session) -> Result<()> {
    if !result.rows.is_empty() || !result.columns.is_empty() {
        match session.format {
            OutputFormat::Table => print_table(&result),
            OutputFormat::Csv => print_csv(&result)?,
            OutputFormat::Json => print_json(&result)?,
        }
    }
    if !session.quiet {
        eprintln!("{}", result.message);
    }
    Ok(())
}

fn print_table(result: &QueryResult) {
    if result.columns.is_empty() {
        return;
    }
    let mut widths: Vec<usize> = result.columns.iter().map(|c| c.len()).collect();
    for row in &result.rows {
        for (i, v) in row.iter().enumerate() {
            if let Some(w) = widths.get_mut(i) {
                *w = (*w).max(v.display().len());
            }
        }
    }
    for (i, c) in result.columns.iter().enumerate() {
        print!(
            "{c:width$}{}",
            if i + 1 == result.columns.len() {
                "\n"
            } else {
                " | "
            },
            width = widths[i]
        );
    }
    for (i, w) in widths.iter().enumerate() {
        print!(
            "{}{}",
            "-".repeat(*w),
            if i + 1 == widths.len() { "\n" } else { "-+-" }
        );
    }
    for row in &result.rows {
        for (i, v) in row.iter().enumerate() {
            print!(
                "{:width$}{}",
                v.display(),
                if i + 1 == row.len() { "\n" } else { " | " },
                width = widths[i]
            );
        }
    }
}

fn print_csv(result: &QueryResult) -> Result<()> {
    let mut w = csv::Writer::from_writer(io::stdout());
    w.write_record(&result.columns)
        .map_err(|e| MiniDbError::Sql(e.to_string()))?;
    for row in &result.rows {
        w.write_record(row.iter().map(Value::display))
            .map_err(|e| MiniDbError::Sql(e.to_string()))?;
    }
    w.flush()?;
    Ok(())
}

fn print_json(result: &QueryResult) -> Result<()> {
    let rows = result
        .rows
        .iter()
        .map(|r| {
            let mut m = serde_json::Map::new();
            for (c, v) in result.columns.iter().zip(r) {
                m.insert(
                    c.clone(),
                    match v {
                        Value::Null => serde_json::Value::Null,
                        Value::Int(i) => serde_json::json!(i),
                        Value::Real(f) => serde_json::json!(f),
                        Value::Text(s) => serde_json::json!(s),
                        Value::Bool(b) => serde_json::json!(b),
                    },
                );
            }
            serde_json::Value::Object(m)
        })
        .collect::<Vec<_>>();
    println!("{}", serde_json::Value::Array(rows));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn format_enum_exists() {
        assert_eq!(OutputFormat::Table, OutputFormat::Table);
    }
}

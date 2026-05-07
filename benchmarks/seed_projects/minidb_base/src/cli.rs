//! Command-line entry point, argument parsing, REPL, and batch runner.
//!
//! The CLI prefers a hand-rolled argument parser so the crate stays
//! dependency-light. Argument semantics:
//!
//! | flag                    | meaning                                  |
//! |-------------------------|------------------------------------------|
//! | `--eval SQL`            | execute `SQL` then exit                  |
//! | `--file PATH`           | execute SQL from `PATH` then exit        |
//! | `--format NAME`         | output format (table/csv/json/ascii-art) |
//! | `--continue-on-error`   | don't abort on first error in batch mode |
//! | `--fast-mode`           | legacy: bypass WAL fsync at commit       |
//! | `--db PATH`             | WAL file location (default: `./minidb.wal`) |
//! | `-h` / `--help`         | print usage                              |
//!
//! Exit codes:
//!
//! * 0 — success
//! * 1 — parse error (SQL or argument)
//! * 2 — runtime / execution error
//! * 3 — I/O, storage, or catalog error

use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::exec::{build_operator, delete::execute_delete, insert::execute_insert, update::execute_update, ExecContext};
use crate::format::{render, OutputFormat};
use crate::parser::ast::Statement;
use crate::parser::parse_many;
use crate::plan::Planner;
use crate::storage::Wal;
use crate::txn::TxnManager;
use crate::types::{Column, Row, Schema, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

/// Exit codes exposed as an enum for clarity.
#[derive(Debug, Clone, Copy)]
pub enum ExitCode {
    Ok = 0,
    ParseError = 1,
    RuntimeError = 2,
    StorageError = 3,
}

impl From<&MiniDbError> for ExitCode {
    fn from(e: &MiniDbError) -> Self {
        match e {
            MiniDbError::Parse(_) | MiniDbError::InvalidArgument(_) => ExitCode::ParseError,
            MiniDbError::Type(_) | MiniDbError::Runtime(_) | MiniDbError::Transaction(_) => {
                ExitCode::RuntimeError
            }
            MiniDbError::Io(_) | MiniDbError::Storage(_) | MiniDbError::Catalog(_) => {
                ExitCode::StorageError
            }
        }
    }
}

/// Parsed CLI arguments.
#[derive(Debug, Clone, Default)]
pub struct CliArgs {
    pub eval: Option<String>,
    pub file: Option<PathBuf>,
    pub format: Option<String>,
    pub continue_on_error: bool,
    pub fast_mode: bool,
    pub db_path: Option<PathBuf>,
    pub help: bool,
}

impl CliArgs {
    pub fn parse<I, S>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut out = Self::default();
        let mut iter = args.into_iter();
        while let Some(raw) = iter.next() {
            let a = raw.as_ref().to_string();
            match a.as_str() {
                "-h" | "--help" => out.help = true,
                "--continue-on-error" => out.continue_on_error = true,
                "--fast-mode" => out.fast_mode = true,
                "--eval" => {
                    let v = iter
                        .next()
                        .ok_or_else(|| MiniDbError::InvalidArgument("--eval expects SQL".into()))?;
                    out.eval = Some(v.as_ref().to_string());
                }
                "--file" => {
                    let v = iter
                        .next()
                        .ok_or_else(|| MiniDbError::InvalidArgument("--file expects path".into()))?;
                    out.file = Some(PathBuf::from(v.as_ref().to_string()));
                }
                "--format" => {
                    let v = iter.next().ok_or_else(|| {
                        MiniDbError::InvalidArgument("--format expects value".into())
                    })?;
                    out.format = Some(v.as_ref().to_string());
                }
                "--db" => {
                    let v = iter
                        .next()
                        .ok_or_else(|| MiniDbError::InvalidArgument("--db expects path".into()))?;
                    out.db_path = Some(PathBuf::from(v.as_ref().to_string()));
                }
                other if other.starts_with("--") => {
                    return Err(MiniDbError::InvalidArgument(format!(
                        "unknown flag `{other}`"
                    )))
                }
                other => {
                    return Err(MiniDbError::InvalidArgument(format!(
                        "unexpected argument `{other}`"
                    )))
                }
            }
        }
        Ok(out)
    }
}

const HELP: &str = "minidb — minimal SQL database

USAGE:
    minidb [OPTIONS]

OPTIONS:
    --eval SQL              Execute SQL, then exit.
    --file PATH             Read SQL from file, then exit.
    --format NAME           Output format: table (default), csv, json, ascii-art.
    --continue-on-error     In batch mode, keep going after errors.
    --fast-mode             Legacy: skip WAL fsync at commit (deprecated).
    --db PATH               Path to the write-ahead log. Default: ./minidb.wal
    -h, --help              Show this help.

REPL META-COMMANDS:
    .help               Show meta-command help.
    .tables             List tables.
    .schema [TABLE]     Show schema(s).
    .indexes            List indexes.
    .format NAME        Switch output format (table/csv/json/ascii-art).
    .quit               Exit.

Exit codes: 0 success | 1 parse error | 2 runtime error | 3 storage/IO error.
";

/// Top-level dispatcher. Returns the process exit code.
pub fn run(args: Vec<String>) -> i32 {
    let cli = match CliArgs::parse(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::ParseError as i32;
        }
    };
    if cli.help {
        println!("{HELP}");
        return ExitCode::Ok as i32;
    }
    match run_inner(cli) {
        Ok(code) => code as i32,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(&e) as i32
        }
    }
}

fn run_inner(cli: CliArgs) -> Result<ExitCode> {
    let db_path = cli
        .db_path
        .clone()
        .unwrap_or_else(|| PathBuf::from("minidb.wal"));
    let wal = Arc::new(Wal::open(&db_path)?);
    let txn = Arc::new(TxnManager::new(wal.clone()).with_fast_mode(cli.fast_mode));
    let catalog = Catalog::new();
    let mut session = Session {
        catalog,
        wal,
        txn,
        format: match cli.format.as_deref() {
            Some(s) => OutputFormat::parse(s)?,
            None => OutputFormat::Table,
        },
    };

    if let Some(sql) = &cli.eval {
        return run_batch(&mut session, sql, cli.continue_on_error);
    }
    if let Some(path) = &cli.file {
        let sql = std::fs::read_to_string(path)?;
        return run_batch(&mut session, &sql, cli.continue_on_error);
    }
    run_repl(&mut session)
}

/// The in-memory state of a running CLI session.
pub struct Session {
    pub catalog: Catalog,
    pub wal: Arc<Wal>,
    pub txn: Arc<TxnManager>,
    pub format: OutputFormat,
}

fn run_batch(session: &mut Session, sql: &str, continue_on_error: bool) -> Result<ExitCode> {
    let stmts = match parse_many(sql) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::ParseError);
        }
    };
    let mut overall = ExitCode::Ok;
    for stmt in stmts {
        if let Err(e) = execute_statement(session, stmt) {
            eprintln!("{e}");
            let code = ExitCode::from(&e);
            overall = code;
            if !continue_on_error {
                return Ok(code);
            }
        }
    }
    Ok(overall)
}

fn run_repl(session: &mut Session) -> Result<ExitCode> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "minidb — type `.help` for meta-commands, Ctrl-D to exit.")?;
    let mut buffer = String::new();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.starts_with('.') {
            if handle_meta(session, &line, &mut out)? {
                break;
            }
            continue;
        }
        buffer.push_str(&line);
        buffer.push(' ');
        if !line.trim_end().ends_with(';') {
            continue;
        }
        let sql = std::mem::take(&mut buffer);
        match parse_many(&sql) {
            Ok(stmts) => {
                for stmt in stmts {
                    if let Err(e) = execute_statement(session, stmt) {
                        eprintln!("{e}");
                    }
                }
            }
            Err(e) => eprintln!("{e}"),
        }
    }
    Ok(ExitCode::Ok)
}

/// Returns `Ok(true)` if the REPL should exit.
fn handle_meta<W: Write>(session: &mut Session, line: &str, out: &mut W) -> Result<bool> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    match parts.as_slice() {
        [".quit"] | [".exit"] => return Ok(true),
        [".help"] => {
            writeln!(out, "{HELP}")?;
        }
        [".tables"] => {
            for name in session.catalog.table_names() {
                writeln!(out, "{name}")?;
            }
        }
        [".indexes"] => {
            for name in session.catalog.index_names() {
                writeln!(out, "{name}")?;
            }
        }
        [".schema"] => {
            for name in session.catalog.table_names() {
                print_schema(session, &name, out)?;
            }
        }
        [".schema", tbl] => {
            print_schema(session, tbl, out)?;
        }
        [".format", fmt] => {
            session.format = OutputFormat::parse(fmt)?;
            writeln!(out, "format = {}", session.format.as_str())?;
        }
        other => {
            writeln!(out, "unknown meta-command: {}", other.join(" "))?;
        }
    }
    Ok(false)
}

fn print_schema<W: Write>(session: &Session, name: &str, out: &mut W) -> Result<()> {
    let tbl = session.catalog.table(name)?;
    write!(out, "CREATE TABLE {name} (")?;
    for (i, c) in tbl.schema.columns.iter().enumerate() {
        if i > 0 {
            write!(out, ", ")?;
        }
        write!(out, "{} {}", c.name, c.ty)?;
        if c.primary_key {
            write!(out, " PRIMARY KEY")?;
        }
        if c.not_null && !c.primary_key {
            write!(out, " NOT NULL")?;
        }
        if c.unique && !c.primary_key {
            write!(out, " UNIQUE")?;
        }
    }
    writeln!(out, ");")?;
    Ok(())
}

/// Dispatch a single parsed statement. Statement summary lines go to
/// stderr; result sets go to stdout.
pub fn execute_statement(session: &mut Session, stmt: Statement) -> Result<()> {
    match stmt {
        Statement::Begin => {
            session.txn.begin()?;
            eprintln!("-- begin");
        }
        Statement::Commit => {
            session.txn.commit()?;
            eprintln!("-- commit");
        }
        Statement::Rollback => {
            session.txn.rollback()?;
            eprintln!("-- rollback");
        }
        Statement::CreateTable(c) => {
            let schema = Schema::new(
                c.columns
                    .iter()
                    .map(|cd| {
                        let mut col = Column::new(&cd.name, cd.ty);
                        col.not_null = cd.not_null;
                        col.unique = cd.unique;
                        col.primary_key = cd.primary_key;
                        col.default = cd.default.as_ref().and_then(|e| {
                            if let crate::parser::ast::Expression::Literal(v) = e {
                                Some(v.clone())
                            } else {
                                None
                            }
                        });
                        col
                    })
                    .collect(),
            );
            session.catalog.create_table(&c.name, schema)?;
            eprintln!("-- created table {}", c.name);
        }
        Statement::DropTable(d) => {
            session.catalog.drop_table(&d.name)?;
            eprintln!("-- dropped table {}", d.name);
        }
        Statement::CreateIndex(ci) => {
            session
                .catalog
                .create_index(&ci.name, &ci.table, &ci.column, ci.unique)?;
            eprintln!("-- created index {} on {}({})", ci.name, ci.table, ci.column);
        }
        Statement::DropIndex(di) => {
            session.catalog.drop_index(&di.name)?;
            eprintln!("-- dropped index {}", di.name);
        }
        Statement::Insert(ins) => {
            let wal = session.wal.clone();
            let txn_manager = session.txn.clone();
            let count = txn_manager.autocommit(|tid| {
                execute_insert(&mut session.catalog, &wal, tid, &ins)
            })?;
            eprintln!("-- inserted {count} row(s)");
        }
        Statement::Update(u) => {
            let wal = session.wal.clone();
            let txn_manager = session.txn.clone();
            let count = txn_manager.autocommit(|tid| {
                execute_update(&mut session.catalog, &wal, tid, &u)
            })?;
            eprintln!("-- updated {count} row(s)");
        }
        Statement::Delete(d) => {
            let wal = session.wal.clone();
            let txn_manager = session.txn.clone();
            let count = txn_manager.autocommit(|tid| {
                execute_delete(&mut session.catalog, &wal, tid, &d)
            })?;
            eprintln!("-- deleted {count} row(s)");
        }
        Statement::Select(sel) => {
            let planner = Planner::new(&session.catalog);
            let (plan, plan_schema) = planner.plan_select(&sel)?;
            let mut ctx = ExecContext::new(&mut session.catalog);
            let mut op = build_operator(plan, plan_schema.clone(), &ctx)?;
            op.open(&mut ctx)?;
            let mut rows: Vec<Row> = Vec::new();
            while let Some(r) = op.next(&mut ctx)? {
                rows.push(r);
            }
            op.close(&mut ctx)?;
            let schema = plan_schema.schema;
            let rendered = render(session.format, &schema, &rows)?;
            let stdout = std::io::stdout();
            let mut w = stdout.lock();
            w.write_all(rendered.as_bytes())?;
            if !rendered.ends_with('\n') {
                writeln!(w)?;
            }
            eprintln!("-- {} row(s)", rows.len());
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn format_row(row: &Row) -> String {
    row.iter()
        .map(|v| match v {
            Value::Null => "NULL".to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join("|")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flags() {
        let args = vec![
            "--eval".to_string(),
            "SELECT 1".to_string(),
            "--format".to_string(),
            "csv".to_string(),
            "--fast-mode".to_string(),
        ];
        let cli = CliArgs::parse(args).unwrap();
        assert_eq!(cli.eval.as_deref(), Some("SELECT 1"));
        assert_eq!(cli.format.as_deref(), Some("csv"));
        assert!(cli.fast_mode);
    }

    #[test]
    fn rejects_unknown_flag() {
        let args = vec!["--nope".to_string()];
        assert!(CliArgs::parse(args).is_err());
    }
}

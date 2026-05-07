use crate::{
    catalog::TableSchema,
    error::{MiniDbError, Result},
    exec,
    parser::{ast::*, parse_sql},
    plan,
    storage::{self, StorageEngine, wal::WalOp},
    txn::TransactionManager,
    types::{Row, Value},
};
use clap::{Parser, ValueEnum};
use is_terminal::IsTerminal;
use std::{
    fs,
    io::{self, Read, Write},
    path::PathBuf,
};

#[derive(Parser)]
#[command(version, about = "Embeddable SQL-subset relational database")]
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
    #[arg(long)]
    init: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    format: OutputFormat,
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
        Ok(()) => 0,
        Err(MiniDbError::Cli(e)) => {
            eprintln!("{e}");
            2
        }
        Err(MiniDbError::Io(e)) => {
            eprintln!("{e}");
            3
        }
        Err(MiniDbError::Storage(e)) => {
            eprintln!("{e}");
            3
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn run_inner() -> Result<()> {
    let args = Args::try_parse().map_err(|e| MiniDbError::Cli(e.to_string()))?;
    if args.init {
        StorageEngine::init(&args.db)?;
        return Ok(());
    }
    let mut session = Session {
        path: args.db.clone(),
        db: StorageEngine::open(&args.db)?,
        txn: TransactionManager::new(),
        format: args.format,
        quiet: args.quiet,
    };
    if let Some(sql) = args.eval {
        session.run_script(&sql, false)?;
    } else if let Some(path) = args.file {
        let sql = fs::read_to_string(path)?;
        session.run_script(&sql, args.continue_on_error)?;
    } else if args.repl || io::stdin().is_terminal() {
        repl(session)?;
    } else {
        let mut sql = String::new();
        io::stdin().read_to_string(&mut sql)?;
        session.run_script(&sql, args.continue_on_error)?;
    }
    Ok(())
}

struct Session {
    path: PathBuf,
    db: StorageEngine,
    txn: TransactionManager,
    format: OutputFormat,
    quiet: bool,
}

#[derive(Debug)]
struct StatementResult {
    columns: Vec<String>,
    rows: Vec<Row>,
    message: String,
}

impl Session {
    fn run_script(&mut self, sql: &str, cont: bool) -> Result<()> {
        for part in split_statements(sql) {
            if part.trim().is_empty() {
                continue;
            }
            if part.trim_start().starts_with('.') {
                if let Err(e) = self.meta(part.trim()) {
                    if cont {
                        eprintln!("{e}");
                        continue;
                    } else {
                        return Err(e);
                    }
                }
                continue;
            }
            match parse_sql(&part).and_then(|stmts| self.run_statements(stmts)) {
                Ok(()) => {}
                Err(e) if cont => eprintln!("{e}"),
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn run_statements(&mut self, stmts: Vec<Statement>) -> Result<()> {
        for stmt in stmts {
            let r = self.execute(stmt)?;
            self.print_result(&r)?;
        }
        Ok(())
    }
    fn execute(&mut self, stmt: Statement) -> Result<StatementResult> {
        match stmt {
            Statement::Begin => {
                self.txn.begin()?;
                Ok(summary("transaction begun", None))
            }
            Statement::Commit => {
                let n = self.txn.commit(&mut self.db)?;
                Ok(summary(&format!("committed {n} operations"), Some(n)))
            }
            Statement::Rollback => {
                let n = self.txn.rollback()?;
                self.db = StorageEngine::open(&self.path)?;
                Ok(summary(&format!("rolled back {n} operations"), Some(0)))
            }
            Statement::CreateTable {
                name,
                columns,
                checks,
            } => self.apply_schema(
                WalOp::CreateTable {
                    schema: TableSchema {
                        name,
                        columns,
                        checks,
                    },
                },
                "table created",
            ),
            Statement::DropTable { name } => {
                self.apply_schema(WalOp::DropTable { name }, "table dropped")
            }
            Statement::CreateIndex {
                name,
                table,
                column,
            } => self.apply_schema(
                WalOp::CreateIndex {
                    index: storage::index_schema(name, table, column),
                },
                "index created",
            ),
            Statement::DropIndex { name } => {
                self.apply_schema(WalOp::DropIndex { name }, "index dropped")
            }
            Statement::Insert {
                table,
                columns,
                values,
            } => self.insert(&table, columns, values),
            Statement::Select(sel) => self.select(sel),
            Statement::Update {
                table,
                assignments,
                selection,
            } => self.update(&table, assignments, selection),
            Statement::Delete { table, selection } => self.delete(&table, selection),
        }
    }
    fn apply_schema(&mut self, op: WalOp, msg: &str) -> Result<StatementResult> {
        self.apply_ops(vec![op])?;
        Ok(summary(msg, Some(0)))
    }
    fn apply_ops(&mut self, ops: Vec<WalOp>) -> Result<()> {
        if self.txn.in_tx() {
            for op in ops {
                self.db.apply_uncommitted(op.clone())?;
                self.txn.push(op);
            }
            Ok(())
        } else {
            self.db.append_tx(&ops)
        }
    }
    fn insert(
        &mut self,
        table: &str,
        columns: Vec<String>,
        exprs: Vec<Expr>,
    ) -> Result<StatementResult> {
        let schema = self.db.catalog().table(table)?.clone();
        let names: Vec<_> = schema.columns.iter().map(|c| c.name.clone()).collect();
        let vals = exprs
            .iter()
            .map(|e| exec::eval(e, &[], &vec![]))
            .collect::<Result<Vec<_>>>()?;
        let mut row = vec![Value::Null; schema.columns.len()];
        if columns.is_empty() {
            if vals.len() != row.len() {
                return Err(MiniDbError::Sql(
                    "value count does not match column count".into(),
                ));
            }
            row = vals;
        } else {
            for (c, v) in columns.iter().zip(vals) {
                let pos = names
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(c))
                    .ok_or_else(|| MiniDbError::Sql(format!("unknown column {c}")))?;
                row[pos] = v;
            }
        }
        for (i, col) in schema.columns.iter().enumerate() {
            if matches!(row[i], Value::Null) {
                if let Some(d) = &col.default {
                    row[i] = d.clone();
                }
            }
            row[i] = row[i].cast_for(&col.ty)?;
        }
        validate_row(&schema, &self.db.table_rows(table)?, None, &row)?;
        self.apply_ops(vec![WalOp::Insert {
            table: table.into(),
            row,
        }])?;
        Ok(summary("1 row affected", Some(1)))
    }
    fn select(&mut self, sel: Select) -> Result<StatementResult> {
        let lp = plan::logical_select(sel);
        let mut pp = plan::physical(&self.db, lp)?.root;
        pp.open()?;
        let columns = pp.schema();
        let mut rows = Vec::new();
        while let Some(r) = pp.next()? {
            rows.push(r.values);
        }
        pp.close()?;
        Ok(StatementResult {
            message: format!("{} rows", rows.len()),
            columns,
            rows,
        })
    }
    fn update(
        &mut self,
        table: &str,
        assigns: Vec<(String, Expr)>,
        selection: Option<Expr>,
    ) -> Result<StatementResult> {
        let schema = self.db.catalog().table(table)?.clone();
        let names = exec::names_for(&schema);
        let mut ops = Vec::new();
        for (id, old) in self.db.table_rows(table)? {
            if selection
                .as_ref()
                .map(|e| exec::eval(e, &names, &old).map(|v| v.as_truth().is_true()))
                .transpose()?
                .map_or(true, |v| v)
            {
                let mut row = old.clone();
                for (c, e) in &assigns {
                    let pos = schema
                        .columns
                        .iter()
                        .position(|x| x.name.eq_ignore_ascii_case(c))
                        .ok_or_else(|| MiniDbError::Sql(format!("unknown column {c}")))?;
                    row[pos] = exec::eval(e, &names, &old)?.cast_for(&schema.columns[pos].ty)?;
                }
                validate_row(&schema, &self.db.table_rows(table)?, Some(id), &row)?;
                ops.push(WalOp::Update {
                    table: table.into(),
                    row_id: id,
                    row,
                });
            }
        }
        let n = ops.len();
        self.apply_ops(ops)?;
        Ok(summary(&format!("{n} rows affected"), Some(n)))
    }
    fn delete(&mut self, table: &str, selection: Option<Expr>) -> Result<StatementResult> {
        let schema = self.db.catalog().table(table)?.clone();
        let names = exec::names_for(&schema);
        let mut ops = Vec::new();
        for (id, row) in self.db.table_rows(table)? {
            if selection
                .as_ref()
                .map(|e| exec::eval(e, &names, &row).map(|v| v.as_truth().is_true()))
                .transpose()?
                .map_or(true, |v| v)
            {
                ops.push(WalOp::Delete {
                    table: table.into(),
                    row_id: id,
                });
            }
        }
        let n = ops.len();
        self.apply_ops(ops)?;
        Ok(summary(&format!("{n} rows affected"), Some(n)))
    }
    fn print_result(&self, r: &StatementResult) -> Result<()> {
        if !r.rows.is_empty() || !r.columns.is_empty() {
            match self.format {
                OutputFormat::Table => print_table(&r.columns, &r.rows)?,
                OutputFormat::Csv => print_csv(&r.columns, &r.rows)?,
                OutputFormat::Json => print_json(&r.columns, &r.rows)?,
            }
        }
        if !self.quiet {
            eprintln!("ok {}", r.message);
        }
        Ok(())
    }
    fn meta(&mut self, cmd: &str) -> Result<()> {
        let parts: Vec<_> = cmd.split_whitespace().collect();
        match parts.first().copied().map_or("", |s| s) {
            ".exit" => std::process::exit(0),
            ".format" => {
                self.format = match parts.get(1).copied() {
                    Some("table") => OutputFormat::Table,
                    Some("csv") => OutputFormat::Csv,
                    Some("json") => OutputFormat::Json,
                    _ => return Err(MiniDbError::Cli("usage: .format table|csv|json".into())),
                };
            }
            ".tables" => {
                for k in self.db.catalog().tables.keys() {
                    println!("{k}");
                }
            }
            ".indexes" => {
                for i in self.db.catalog().indexes.values() {
                    println!("{} ON {}({})", i.name, i.table, i.column);
                }
            }
            ".schema" => {
                for t in self.db.catalog().tables.values() {
                    println!("CREATE TABLE {} (...);", t.name);
                }
            }
            _ => return Err(MiniDbError::Cli(format!("unknown meta-command {cmd}"))),
        }
        Ok(())
    }
}

fn validate_row(
    schema: &TableSchema,
    existing: &[(u64, Row)],
    self_id: Option<u64>,
    row: &Row,
) -> Result<()> {
    let names = exec::names_for(schema);
    for (i, c) in schema.columns.iter().enumerate() {
        if c.not_null && matches!(row[i], Value::Null) {
            return Err(MiniDbError::Sql(format!("{} may not be NULL", c.name)));
        }
        if c.unique || c.primary_key {
            for (id, erow) in existing {
                if Some(*id) != self_id
                    && erow.get(i) == row.get(i)
                    && !matches!(row.get(i), Some(Value::Null))
                {
                    return Err(MiniDbError::Sql(format!(
                        "unique constraint failed: {}",
                        c.name
                    )));
                }
            }
        }
    }
    for chk in &schema.checks {
        if !exec::eval(chk, &names, row)?.as_truth().is_true() {
            return Err(MiniDbError::Sql("check constraint failed".into()));
        }
    }
    Ok(())
}
fn summary(message: &str, _affected: Option<usize>) -> StatementResult {
    StatementResult {
        columns: Vec::new(),
        rows: Vec::new(),
        message: message.into(),
    }
}
fn split_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    for ch in sql.chars() {
        if ch == '\'' {
            in_str = !in_str;
        }
        if ch == ';' && !in_str {
            cur.push(ch);
            out.push(cur.clone());
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}
fn print_table(cols: &[String], rows: &[Row]) -> Result<()> {
    let mut widths: Vec<usize> = cols.iter().map(|c| c.len()).collect();
    for r in rows {
        for (i, v) in r.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(v.to_string().len());
            }
        }
    }
    if !cols.is_empty() {
        for (i, c) in cols.iter().enumerate() {
            print!(
                "{c:width$}{}",
                if i + 1 == cols.len() { "\n" } else { " | " },
                width = widths[i]
            );
        }
    }
    for r in rows {
        for (i, v) in r.iter().enumerate() {
            print!(
                "{v:width$}{}",
                if i + 1 == r.len() { "\n" } else { " | " },
                width = widths[i]
            );
        }
    }
    io::stdout().flush()?;
    Ok(())
}
fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.into()
    }
}
fn print_csv(cols: &[String], rows: &[Row]) -> Result<()> {
    println!(
        "{}",
        cols.iter()
            .map(|c| csv_cell(c))
            .collect::<Vec<_>>()
            .join(",")
    );
    for r in rows {
        println!(
            "{}",
            r.iter()
                .map(|v| csv_cell(&v.to_string()))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    Ok(())
}
fn print_json(cols: &[String], rows: &[Row]) -> Result<()> {
    let xs: Vec<_> = rows
        .iter()
        .map(|r| {
            cols.iter()
                .zip(r)
                .map(|(c, v)| (c.clone(), json_value(v)))
                .collect::<serde_json::Map<_, _>>()
        })
        .collect();
    println!("{}", serde_json::to_string(&xs)?);
    Ok(())
}
fn json_value(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Int(i) => (*i).into(),
        Value::Real(f) => serde_json::json!(f),
        Value::Text(s) => s.clone().into(),
        Value::Bool(b) => (*b).into(),
    }
}

fn repl(mut session: Session) -> Result<()> {
    let mut buf = String::new();
    loop {
        print!("minidb> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        buf.push_str(&line);
        if line.trim_start().starts_with('.') || buf.trim_end().ends_with(';') {
            if let Err(e) = session.run_script(&buf, false) {
                eprintln!("{e}");
            }
            buf.clear();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splits_semicolons() {
        assert_eq!(split_statements("select ';'; select 1;").len(), 2);
    }
}

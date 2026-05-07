use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;

use crate::error::{MiniDbError, Result};
use crate::parser::{
    ast::{MetaCommand, Statement},
    parse,
};
use crate::plan::{apply_statement_without_wal, StatementOutcome};
use crate::txn::{DatabaseState, TransactionManager};
use clap::Parser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl OutputFormat {
    fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "csv" => Ok(OutputFormat::Csv),
            "json" => Ok(OutputFormat::Json),
            other => Err(MiniDbError::Cli(format!("unknown output format {other}"))),
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "minidb",
    version,
    about = "Embeddable SQL-subset relational database"
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
    #[arg(long, value_parser = ["table", "csv", "json"], default_value = "table")]
    format: String,
    #[arg(long)]
    continue_on_error: bool,
    #[arg(long)]
    quiet: bool,
    #[arg(long)]
    init: bool,
}

pub fn run() -> Result<()> {
    env_logger::builder().format_timestamp(None).try_init().ok();
    let args = Args::try_parse().map_err(|err| MiniDbError::Cli(err.to_string()))?;
    if args.init {
        init_database(&args.db)?;
        return Ok(());
    }
    let mut session = Session::open(args.db, OutputFormat::parse(&args.format)?, args.quiet)?;
    if let Some(sql) = args.eval {
        session.execute_script(&sql, args.continue_on_error)?;
        return Ok(());
    }
    if let Some(path) = args.file {
        let script = fs::read_to_string(path)?;
        session.execute_script(&script, args.continue_on_error)?;
        return Ok(());
    }
    if args.repl || io::stdin().is_terminal() {
        session.repl()?;
        return Ok(());
    }
    Ok(())
}

fn init_database(path: &PathBuf) -> Result<()> {
    let manager = TransactionManager::open(path.clone())?;
    let snapshot = bincode::serialize(&DatabaseState::default())
        .map_err(|err| MiniDbError::Storage(err.to_string()))?;
    manager.page_manager.store_snapshot(0, &snapshot)?;
    manager.wal.truncate()?;
    Ok(())
}

struct Session {
    tx: TransactionManager,
    format: OutputFormat,
    quiet: bool,
}

impl Session {
    fn open(path: PathBuf, format: OutputFormat, quiet: bool) -> Result<Self> {
        let tx = TransactionManager::open(path)?;
        Ok(Self { tx, format, quiet })
    }

    fn repl(&mut self) -> Result<()> {
        let mut buffer = String::new();
        let mut stdin = io::stdin().lock();
        let mut line = String::new();
        loop {
            line.clear();
            let n = stdin.read_line(&mut line)?;
            if n == 0 {
                return Ok(());
            }
            buffer.push_str(&line);
            if Self::looks_complete(&buffer) {
                self.execute_script(&buffer, false)?;
                buffer.clear();
            }
        }
    }

    fn looks_complete(buffer: &str) -> bool {
        let trimmed = buffer.trim_end();
        trimmed.ends_with(';') || trimmed.starts_with('.')
    }

    fn execute_script(&mut self, script: &str, continue_on_error: bool) -> Result<()> {
        let statements = parse(script)?;
        let mut saw_error = false;
        for statement in statements {
            let logged_sql = format_statement(&statement);
            match self.execute_statement(&logged_sql, &statement) {
                Ok(should_exit) => {
                    if should_exit {
                        return Ok(());
                    }
                }
                Err(err) => {
                    saw_error = true;
                    eprintln!("{err}");
                    if !continue_on_error {
                        return Err(err);
                    }
                }
            }
        }
        if saw_error && continue_on_error {
            return Err(MiniDbError::Runtime("one or more statements failed".into()));
        }
        Ok(())
    }

    fn execute_statement(&mut self, sql: &str, statement: &Statement) -> Result<bool> {
        match statement {
            Statement::Begin => {
                self.tx.begin()?;
                self.summary("ok");
                Ok(false)
            }
            Statement::Commit => {
                self.tx.commit()?;
                self.summary("ok");
                Ok(false)
            }
            Statement::Rollback => {
                self.tx.rollback()?;
                self.summary("ok");
                Ok(false)
            }
            Statement::Meta(MetaCommand::Format { kind }) => {
                self.format = OutputFormat::parse(kind)?;
                self.summary("ok");
                Ok(false)
            }
            Statement::Meta(MetaCommand::Exit) => Ok(true),
            _ => {
                let outcome = self
                    .tx
                    .execute_in_txn(sql, |state| apply_statement_without_wal(state, statement))?;
                self.render_outcome(outcome)?;
                Ok(false)
            }
        }
    }

    fn render_outcome(&mut self, outcome: StatementOutcome) -> Result<()> {
        match outcome {
            StatementOutcome::Query(result) => {
                self.print_result(&result)?;
                let suffix = if result.rows.len() == 1 {
                    "row"
                } else {
                    "rows"
                };
                self.summary(&format!("ok {} {suffix}", result.rows.len()));
            }
            StatementOutcome::Affected(count) => {
                let suffix = if count == 1 {
                    "row affected"
                } else {
                    "rows affected"
                };
                self.summary(&format!("ok {count} {suffix}"));
            }
            StatementOutcome::Message(message) => {
                if !message.trim().is_empty() {
                    print!("{message}");
                    io::stdout().flush()?;
                }
                self.summary("ok");
            }
            StatementOutcome::None => self.summary("ok"),
        }
        Ok(())
    }

    fn print_result(&self, result: &crate::types::ResultSet) -> Result<()> {
        match self.format {
            OutputFormat::Table => self.print_table(result)?,
            OutputFormat::Csv => self.print_csv(result)?,
            OutputFormat::Json => self.print_json(result)?,
        }
        Ok(())
    }

    fn print_table(&self, result: &crate::types::ResultSet) -> Result<()> {
        let headers: Vec<String> = result
            .columns
            .iter()
            .map(|c| match &c.table {
                Some(table) => format!("{table}.{}", c.name),
                None => c.name.clone(),
            })
            .collect();
        let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
        for row in &result.rows {
            for (idx, value) in row.iter().enumerate() {
                widths[idx] = widths[idx].max(value.to_string().len());
            }
        }
        let mut out = String::new();
        if !headers.is_empty() {
            out.push_str(&render_row(&headers, &widths));
            out.push('\n');
            out.push_str(&render_separator(&widths));
            out.push('\n');
        }
        for row in &result.rows {
            let cells: Vec<String> = row.iter().map(|value| value.to_string()).collect();
            out.push_str(&render_row(&cells, &widths));
            out.push('\n');
        }
        print!("{out}");
        io::stdout().flush()?;
        Ok(())
    }

    fn print_csv(&self, result: &crate::types::ResultSet) -> Result<()> {
        let headers: Vec<String> = result
            .columns
            .iter()
            .map(|c| match &c.table {
                Some(table) => format!("{table}.{}", c.name),
                None => c.name.clone(),
            })
            .collect();
        let mut out = String::new();
        out.push_str(&csv_line(&headers));
        out.push('\n');
        for row in &result.rows {
            let cells: Vec<String> = row.iter().map(|value| value.to_string()).collect();
            out.push_str(&csv_line(&cells));
            out.push('\n');
        }
        print!("{out}");
        io::stdout().flush()?;
        Ok(())
    }

    fn print_json(&self, result: &crate::types::ResultSet) -> Result<()> {
        let mut rows = Vec::new();
        for row in &result.rows {
            let mut obj = serde_json::Map::new();
            for (idx, value) in row.iter().enumerate() {
                let col = &result.columns[idx];
                let key = match &col.table {
                    Some(table) => format!("{table}.{}", col.name),
                    None => col.name.clone(),
                };
                obj.insert(key, serde_json::Value::String(value.to_string()));
            }
            rows.push(serde_json::Value::Object(obj));
        }
        println!(
            "{}",
            serde_json::to_string(&rows).map_err(|err| MiniDbError::Storage(err.to_string()))?
        );
        Ok(())
    }

    fn summary(&self, message: &str) {
        if !self.quiet {
            eprintln!("{message}");
        }
    }
}

fn render_row(cells: &[String], widths: &[usize]) -> String {
    let mut out = String::new();
    out.push('|');
    for (idx, cell) in cells.iter().enumerate() {
        out.push(' ');
        out.push_str(&format!("{cell:<width$}", width = widths[idx]));
        out.push(' ');
        out.push('|');
    }
    out
}

fn render_separator(widths: &[usize]) -> String {
    let mut out = String::new();
    out.push('+');
    for width in widths {
        out.push_str(&"-".repeat(width + 2));
        out.push('+');
    }
    out
}

fn csv_line(cells: &[String]) -> String {
    cells
        .iter()
        .map(|cell| {
            if cell.contains(',') || cell.contains('"') || cell.contains('\n') {
                format!("\"{}\"", cell.replace('"', "\"\""))
            } else {
                cell.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn format_statement(statement: &Statement) -> String {
    match statement {
        Statement::CreateTable {
            name,
            columns,
            table_constraints,
        } => {
            let mut parts = Vec::new();
            for col in columns {
                let mut part = format!("{} {}", col.name, col.col_type.name());
                if col.constraints.primary_key {
                    part.push_str(" PRIMARY KEY");
                }
                if col.constraints.not_null {
                    part.push_str(" NOT NULL");
                }
                if col.constraints.unique {
                    part.push_str(" UNIQUE");
                }
                if let Some(default) = &col.constraints.default {
                    part.push_str(&format!(" DEFAULT {}", literal_sql(default)));
                }
                parts.push(part);
            }
            for constraint in table_constraints {
                match constraint {
                    crate::types::TableConstraint::Check(expr) => {
                        parts.push(format!("CHECK ({expr:?})"));
                    }
                }
            }
            format!("CREATE TABLE {} ({});", name, parts.join(", "))
        }
        Statement::DropTable { name } => format!("DROP TABLE {};", name),
        Statement::CreateIndex {
            name,
            table,
            column,
        } => format!("CREATE INDEX {} ON {} ({});", name, table, column),
        Statement::DropIndex { name } => format!("DROP INDEX {};", name),
        Statement::Insert {
            table,
            columns,
            values,
        } => {
            let mut sql = format!("INSERT INTO {}", table);
            if let Some(columns) = columns {
                sql.push_str(&format!(" ({})", columns.join(", ")));
            }
            sql.push_str(" VALUES (");
            sql.push_str(&values.iter().map(expr_sql).collect::<Vec<_>>().join(", "));
            sql.push_str(");");
            sql
        }
        Statement::Select(select) => {
            let projection = select
                .projection
                .iter()
                .map(|item| match item {
                    crate::parser::ast::SelectItem::Wildcard => "*".to_string(),
                    crate::parser::ast::SelectItem::Expr { expr, alias } => {
                        let mut s = expr_sql(expr);
                        if let Some(alias) = alias {
                            s.push_str(&format!(" AS {alias}"));
                        }
                        s
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            let mut sql = format!("SELECT {} FROM {}", projection, table_ref_sql(&select.from));
            for join in &select.joins {
                sql.push_str(&format!(
                    " INNER JOIN {} ON {}",
                    table_ref_sql(&join.table),
                    expr_sql(&join.on)
                ));
            }
            if let Some(selection) = &select.selection {
                sql.push_str(&format!(" WHERE {}", expr_sql(selection)));
            }
            if let Some(order) = &select.order_by {
                sql.push_str(" ORDER BY ");
                sql.push_str(
                    &order
                        .iter()
                        .map(|ob| {
                            format!(
                                "{} {}",
                                expr_sql(&ob.expr),
                                if ob.ascending { "ASC" } else { "DESC" }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
            if let Some(limit) = &select.limit {
                sql.push_str(&format!(" LIMIT {}", limit.limit));
                if limit.offset > 0 {
                    sql.push_str(&format!(" OFFSET {}", limit.offset));
                }
            }
            sql.push(';');
            sql
        }
        Statement::Update {
            table,
            assignments,
            selection,
        } => {
            let mut sql = format!("UPDATE {} SET ", table);
            sql.push_str(
                &assignments
                    .iter()
                    .map(|(col, expr)| format!("{col} = {}", expr_sql(expr)))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            if let Some(selection) = selection {
                sql.push_str(&format!(" WHERE {}", expr_sql(selection)));
            }
            sql.push(';');
            sql
        }
        Statement::Delete { table, selection } => {
            let mut sql = format!("DELETE FROM {}", table);
            if let Some(selection) = selection {
                sql.push_str(&format!(" WHERE {}", expr_sql(selection)));
            }
            sql.push(';');
            sql
        }
        Statement::Begin => "BEGIN;".to_string(),
        Statement::Commit => "COMMIT;".to_string(),
        Statement::Rollback => "ROLLBACK;".to_string(),
        Statement::Meta(MetaCommand::Schema { table }) => match table {
            Some(table) => format!(".schema {table}"),
            None => ".schema".to_string(),
        },
        Statement::Meta(MetaCommand::Tables) => ".tables".to_string(),
        Statement::Meta(MetaCommand::Indexes) => ".indexes".to_string(),
        Statement::Meta(MetaCommand::Format { kind }) => format!(".format {kind}"),
        Statement::Meta(MetaCommand::Exit) => ".exit".to_string(),
    }
}

fn table_ref_sql(table: &crate::parser::ast::TableRef) -> String {
    match &table.alias {
        Some(alias) => format!("{} AS {}", table.name, alias),
        None => table.name.clone(),
    }
}

fn expr_sql(expr: &crate::parser::ast::Expr) -> String {
    use crate::parser::ast::{BinaryOp, Expr, UnaryOp};
    match expr {
        Expr::Literal(literal) => literal_sql(literal),
        Expr::ColumnRef { table, column } => match table {
            Some(table) => format!("{table}.{column}"),
            None => column.clone(),
        },
        Expr::Star => "*".to_string(),
        Expr::QualifiedStar { table } => format!("{table}.*"),
        Expr::Unary { op, expr } => match op {
            UnaryOp::Neg => format!("-{}", expr_sql(expr)),
            UnaryOp::Not => format!("NOT {}", expr_sql(expr)),
            UnaryOp::IsNull => format!("{} IS NULL", expr_sql(expr)),
            UnaryOp::IsNotNull => format!("{} IS NOT NULL", expr_sql(expr)),
        },
        Expr::Binary { left, op, right } => {
            let op_str = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                BinaryOp::Concat => "||",
                BinaryOp::Eq => "=",
                BinaryOp::Neq => "<>",
                BinaryOp::Lt => "<",
                BinaryOp::Lte => "<=",
                BinaryOp::Gt => ">",
                BinaryOp::Gte => ">=",
                BinaryOp::And => "AND",
                BinaryOp::Or => "OR",
            };
            format!("{} {} {}", expr_sql(left), op_str, expr_sql(right))
        }
        Expr::Function { name, args } => {
            format!(
                "{}({})",
                name,
                args.iter().map(expr_sql).collect::<Vec<_>>().join(", ")
            )
        }
    }
}

fn literal_sql(literal: &crate::types::Literal) -> String {
    match literal {
        crate::types::Literal::Null => "NULL".to_string(),
        crate::types::Literal::Int(v) => v.to_string(),
        crate::types::Literal::Real(v) => v.to_string(),
        crate::types::Literal::Text(v) => format!("'{}'", v.replace('\'', "''")),
        crate::types::Literal::Bool(v) => v.to_string(),
    }
}

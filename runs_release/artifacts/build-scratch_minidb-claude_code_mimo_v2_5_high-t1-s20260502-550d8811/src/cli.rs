use crate::catalog::{Catalog, TableInfo};
use crate::error::{MiniDbError, Result};
use crate::exec::operators::*;
use crate::exec::ExecResult;
use crate::lexer::Lexer;
use crate::parser::ast::*;
use crate::parser::Parser;
use crate::storage::heap::HeapFile;
use crate::storage::wal::Wal;
use crate::txn::TransactionManager;
use crate::types::{ColumnDef, Row, RowId, Value};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

pub enum UndoEntry {
    Insert {
        table: String,
        row_id: RowId,
    },
    Update {
        table: String,
        row_id: RowId,
        old_row: Row,
    },
    Delete {
        table: String,
        row_id: RowId,
        old_row: Row,
    },
}

pub struct MiniDb {
    _db_path: PathBuf,
    pub(crate) catalog: Catalog,
    pub(crate) heap: HeapFile,
    pub(crate) wal: Wal,
    pub(crate) txn_mgr: TransactionManager,
    format: OutputFormat,
    pub(crate) quiet: bool,
    undo_log: Vec<UndoEntry>,
}

impl MiniDb {
    pub fn open(db_path: &Path, quiet: bool) -> Result<Self> {
        let wal_path = db_path.with_extension("wal");
        let heap_path = db_path.with_extension("dat");

        let catalog = Catalog::new();
        let wal = Wal::open(&wal_path)?;
        let mut txn_mgr = TransactionManager::new();

        // Replay WAL for crash recovery
        let records = wal.read_all_records()?;
        txn_mgr.replay_wal(&records);
        log::info!("WAL replay: {} records", records.len());

        let heap = HeapFile::open(&heap_path)?;

        let db = MiniDb {
            _db_path: db_path.to_path_buf(),
            catalog,
            heap,
            wal,
            txn_mgr,
            format: OutputFormat::Table,
            quiet,
            undo_log: Vec::new(),
        };

        Ok(db)
    }

    pub fn set_format(&mut self, fmt: OutputFormat) {
        self.format = fmt;
    }

    pub fn execute_sql(&mut self, sql: &str) -> Result<ExecResult> {
        let mut lexer = Lexer::new(sql);
        let tokens = lexer.tokenize()?;
        let mut parser = Parser::new(tokens);
        let stmts = parser.parse_statements()?;

        let mut last_result = None;
        for stmt in stmts {
            last_result = Some(self.execute_statement(&stmt)?);
        }
        last_result.ok_or_else(|| MiniDbError::Sql("no statement to execute".into()))
    }

    pub fn execute_statement(&mut self, stmt: &Statement) -> Result<ExecResult> {
        match stmt {
            Statement::Begin => {
                self.txn_mgr.begin()?;
                self.undo_log.clear();
                Ok(ExecResult::Changed { rows_affected: 0 })
            }
            Statement::Commit => {
                let tx_id = self
                    .txn_mgr
                    .active_tx_id()
                    .ok_or_else(|| MiniDbError::Transaction("no active transaction".into()))?;
                self.txn_mgr.commit(tx_id, &mut self.wal)?;
                self.undo_log.clear();
                Ok(ExecResult::Changed { rows_affected: 0 })
            }
            Statement::Rollback => {
                let tx_id = self
                    .txn_mgr
                    .active_tx_id()
                    .ok_or_else(|| MiniDbError::Transaction("no active transaction".into()))?;

                // Replay undo log in reverse
                for entry in self.undo_log.drain(..).rev() {
                    match entry {
                        UndoEntry::Insert { table, row_id } => {
                            let _ = self.heap.delete_row(&table, &row_id);
                        }
                        UndoEntry::Update {
                            table,
                            row_id,
                            old_row,
                        } => {
                            let _ = self.heap.update_row(&table, &row_id, &old_row);
                        }
                        UndoEntry::Delete {
                            table,
                            row_id: _,
                            old_row,
                        } => {
                            let _ = self.heap.insert_row(&table, &old_row);
                        }
                    }
                }

                self.txn_mgr.rollback(tx_id, &mut self.wal)?;
                Ok(ExecResult::Changed { rows_affected: 0 })
            }
            Statement::CreateTable(create) => {
                let info = TableInfo {
                    name: create.name.clone(),
                    columns: create.columns.clone(),
                };
                self.catalog.create_table(info)?;
                let schema_data = bincode_serialize_columns(&create.columns);
                self.wal.append(crate::storage::wal::WalOp::CreateTable {
                    tx_id: self.txn_mgr.active_tx_id().unwrap_or(0),
                    table: create.name.clone(),
                    schema_data,
                })?;
                Ok(ExecResult::Changed { rows_affected: 0 })
            }
            Statement::DropTable(drop) => {
                self.catalog.drop_table(&drop.name)?;
                self.wal.append(crate::storage::wal::WalOp::DropTable {
                    tx_id: self.txn_mgr.active_tx_id().unwrap_or(0),
                    table: drop.name.clone(),
                })?;
                Ok(ExecResult::Changed { rows_affected: 0 })
            }
            Statement::CreateIndex(create) => {
                self.catalog.create_index(
                    create.name.clone(),
                    create.table.clone(),
                    create.column.clone(),
                )?;
                self.wal.append(crate::storage::wal::WalOp::CreateIndex {
                    tx_id: self.txn_mgr.active_tx_id().unwrap_or(0),
                    index_name: create.name.clone(),
                    table: create.table.clone(),
                    column: create.column.clone(),
                })?;
                Ok(ExecResult::Changed { rows_affected: 0 })
            }
            Statement::DropIndex(drop) => {
                self.catalog.drop_index(&drop.name)?;
                self.wal.append(crate::storage::wal::WalOp::DropIndex {
                    tx_id: self.txn_mgr.active_tx_id().unwrap_or(0),
                    index_name: drop.name.clone(),
                })?;
                Ok(ExecResult::Changed { rows_affected: 0 })
            }
            Statement::Select(sel) => self.execute_select(sel),
            Statement::Insert(ins) => {
                let in_txn = self.txn_mgr.in_transaction();
                let old_count = if in_txn {
                    self.heap.scan_table(&ins.table)?.len()
                } else {
                    0
                };

                let mut op =
                    InsertOperator::new(ins.table.clone(), ins.columns.clone(), ins.rows.clone());
                let mut ctx = ExecContext {
                    catalog: &mut self.catalog,
                    heap: &mut self.heap,
                };
                op.execute(&mut ctx)?;
                let affected = op.rows_affected;

                if in_txn {
                    // Record undo: find the newly inserted row(s)
                    let all_rows = self.heap.scan_table(&ins.table)?;
                    // The new rows are the last `affected` rows
                    let new_rows: Vec<_> = all_rows.into_iter().skip(old_count).collect();
                    for (rid, _) in new_rows {
                        self.undo_log.push(UndoEntry::Insert {
                            table: ins.table.clone(),
                            row_id: rid,
                        });
                    }
                }

                if !self.txn_mgr.in_transaction() {
                    self.wal
                        .append(crate::storage::wal::WalOp::Commit { tx_id: 0 })?;
                }

                Ok(ExecResult::Changed {
                    rows_affected: affected,
                })
            }
            Statement::Update(upd) => {
                let in_txn = self.txn_mgr.in_transaction();

                // Capture old state for undo
                let old_rows: Vec<(RowId, Row)> = if in_txn {
                    let table_info = self.catalog.get_table(&upd.table)?;
                    let col_names: Vec<String> =
                        table_info.columns.iter().map(|c| c.name.clone()).collect();
                    let all_rows = self.heap.scan_table(&upd.table)?;
                    if let Some(ref where_expr) = upd.where_clause {
                        all_rows
                            .into_iter()
                            .filter(|(_, row)| {
                                evaluate_expr(where_expr, row, &col_names)
                                    .map(|v| v.as_bool().unwrap_or(false))
                                    .unwrap_or(false)
                            })
                            .collect()
                    } else {
                        all_rows
                    }
                } else {
                    vec![]
                };

                let table_info = self.catalog.get_table(&upd.table)?.clone();
                let col_names: Vec<String> =
                    table_info.columns.iter().map(|c| c.name.clone()).collect();
                let mut op = UpdateOperator::new(
                    upd.table.clone(),
                    upd.assignments.clone(),
                    upd.where_clause.clone(),
                    col_names,
                );
                let mut ctx = ExecContext {
                    catalog: &mut self.catalog,
                    heap: &mut self.heap,
                };
                op.execute(&mut ctx)?;
                let affected = op.rows_affected;

                if in_txn {
                    for (rid, old_row) in old_rows {
                        self.undo_log.push(UndoEntry::Update {
                            table: upd.table.clone(),
                            row_id: rid,
                            old_row,
                        });
                    }
                }

                if !self.txn_mgr.in_transaction() {
                    self.wal
                        .append(crate::storage::wal::WalOp::Commit { tx_id: 0 })?;
                }

                Ok(ExecResult::Changed {
                    rows_affected: affected,
                })
            }
            Statement::Delete(del) => {
                let in_txn = self.txn_mgr.in_transaction();

                // Capture old state for undo
                let old_rows: Vec<(RowId, Row)> = if in_txn {
                    let table_info = self.catalog.get_table(&del.table)?;
                    let col_names: Vec<String> =
                        table_info.columns.iter().map(|c| c.name.clone()).collect();
                    let all_rows = self.heap.scan_table(&del.table)?;
                    if let Some(ref where_expr) = del.where_clause {
                        all_rows
                            .into_iter()
                            .filter(|(_, row)| {
                                evaluate_expr(where_expr, row, &col_names)
                                    .map(|v| v.as_bool().unwrap_or(false))
                                    .unwrap_or(false)
                            })
                            .collect()
                    } else {
                        all_rows
                    }
                } else {
                    vec![]
                };

                let table_info = self.catalog.get_table(&del.table)?.clone();
                let col_names: Vec<String> =
                    table_info.columns.iter().map(|c| c.name.clone()).collect();
                let mut op =
                    DeleteOperator::new(del.table.clone(), del.where_clause.clone(), col_names);
                let mut ctx = ExecContext {
                    catalog: &mut self.catalog,
                    heap: &mut self.heap,
                };
                op.execute(&mut ctx)?;
                let affected = op.rows_affected;

                if in_txn {
                    for (rid, old_row) in old_rows {
                        self.undo_log.push(UndoEntry::Delete {
                            table: del.table.clone(),
                            row_id: rid,
                            old_row,
                        });
                    }
                }

                if !self.txn_mgr.in_transaction() {
                    self.wal
                        .append(crate::storage::wal::WalOp::Commit { tx_id: 0 })?;
                }

                Ok(ExecResult::Changed {
                    rows_affected: affected,
                })
            }
        }
    }

    fn execute_select(&mut self, sel: &SelectStatement) -> Result<ExecResult> {
        // Handle FROM-less SELECT (e.g., SELECT 1+2)
        if sel.from.is_empty() {
            let mut proj_columns = Vec::new();
            let mut row = Vec::new();
            for item in &sel.projection {
                match item {
                    SelectItem::Star => {}
                    SelectItem::Expr { expr, alias } => {
                        let name = alias.clone().unwrap_or_else(|| expr_display(expr));
                        proj_columns.push(name);
                        let val = evaluate_expr(expr, &vec![], &[])?;
                        row.push(val);
                    }
                }
            }
            return Ok(ExecResult::Rows {
                columns: proj_columns,
                rows: vec![row],
            });
        }

        let table_name = if let Some(FromItem::Table { name, alias: _ }) = sel.from.first() {
            name.clone()
        } else {
            return Err(MiniDbError::Sql(
                "only single table SELECT is supported".into(),
            ));
        };

        let table_info = self.catalog.get_table(&table_name)?.clone();
        let col_names: Vec<String> = table_info.columns.iter().map(|c| c.name.clone()).collect();

        // Build projection column list
        let mut proj_columns = Vec::new();
        for item in &sel.projection {
            match item {
                SelectItem::Star => proj_columns.extend(col_names.iter().cloned()),
                SelectItem::Expr { expr, alias } => {
                    let name = if let Some(a) = alias {
                        a.clone()
                    } else {
                        expr_display(expr)
                    };
                    proj_columns.push(name);
                }
            }
        }

        // Get all rows
        let mut all_rows = self.heap.scan_table(&table_name)?;

        // Apply WHERE filter
        if let Some(ref where_expr) = sel.where_clause {
            let mut filtered = Vec::new();
            for (rid, row) in &all_rows {
                let val = evaluate_expr(where_expr, row, &col_names)?;
                if val.as_bool().unwrap_or(false) {
                    filtered.push((*rid, row.clone()));
                }
            }
            all_rows = filtered;
        }

        // Check if we can use an index for the filter (already applied above for simplicity)

        // Apply ORDER BY
        if !sel.order_by.is_empty() {
            let order_by = sel.order_by.clone();
            let cn = col_names.clone();
            all_rows.sort_by(|a, b| {
                for item in &order_by {
                    let va = evaluate_expr(&item.expr, &a.1, &cn).unwrap_or(Value::Null);
                    let vb = evaluate_expr(&item.expr, &b.1, &cn).unwrap_or(Value::Null);
                    let cmp = match &item.direction {
                        OrderDirection::Asc => va.cmp(&vb),
                        OrderDirection::Desc => vb.cmp(&va),
                    };
                    if cmp != std::cmp::Ordering::Equal {
                        return cmp;
                    }
                }
                std::cmp::Ordering::Equal
            });
        }

        // Apply LIMIT/OFFSET
        if let Some(ref limit) = sel.limit {
            let count = evaluate_expr(&limit.count, &vec![], &[])?
                .as_int()
                .unwrap_or(i64::MAX) as usize;
            let offset = if let Some(ref off) = limit.offset {
                evaluate_expr(off, &vec![], &[])?.as_int().unwrap_or(0) as usize
            } else {
                0
            };
            all_rows = all_rows.into_iter().skip(offset).take(count).collect();
        }

        // Apply projection
        let mut result_rows = Vec::new();
        for (_rid, row) in &all_rows {
            let mut projected = Vec::new();
            for item in &sel.projection {
                match item {
                    SelectItem::Star => projected.extend(row.iter().cloned()),
                    SelectItem::Expr { expr, .. } => {
                        let val = evaluate_expr(expr, row, &col_names)?;
                        projected.push(val);
                    }
                }
            }
            result_rows.push(projected);
        }

        Ok(ExecResult::Rows {
            columns: proj_columns,
            rows: result_rows,
        })
    }

    pub fn format_result(&self, result: &ExecResult) -> String {
        match result {
            ExecResult::Rows { columns, rows } => match &self.format {
                OutputFormat::Table => format_table(columns, rows),
                OutputFormat::Csv => format_csv(columns, rows),
                OutputFormat::Json => format_json(columns, rows),
            },
            ExecResult::Changed { .. } => String::new(),
        }
    }

    pub fn meta_command(&mut self, cmd: &str) -> Result<Option<String>> {
        let cmd = cmd.trim();
        if cmd == ".schema" {
            let mut output = String::new();
            for name in self.catalog.all_table_names() {
                if let Ok(s) = self.catalog.schema_string(&name) {
                    output.push_str(&s);
                    output.push_str(";\n\n");
                }
            }
            Ok(Some(output))
        } else if cmd == ".tables" {
            let names = self.catalog.all_table_names();
            Ok(Some(names.join("\n")))
        } else if cmd == ".indexes" {
            let names = self.catalog.all_index_names();
            Ok(Some(names.join("\n")))
        } else if cmd.starts_with(".format ") {
            let fmt = cmd[8..].trim();
            match fmt {
                "table" => self.format = OutputFormat::Table,
                "csv" => self.format = OutputFormat::Csv,
                "json" => self.format = OutputFormat::Json,
                _ => return Err(MiniDbError::Sql(format!("unknown format: {}", fmt))),
            }
            Ok(Some(format!("format set to {}", fmt)))
        } else if cmd == ".exit" {
            Ok(None)
        } else {
            Err(MiniDbError::Sql(format!("unknown meta-command: {}", cmd)))
        }
    }
}

fn expr_display(expr: &Expr) -> String {
    match expr {
        Expr::Literal(lit) => format!("{}", lit.to_value()),
        Expr::ColumnRef { table, column } => match table {
            Some(t) => format!("{}.{}", t, column),
            None => column.clone(),
        },
        Expr::Function { name, args } => {
            let arg_strs: Vec<String> = args.iter().map(|a| expr_display(a)).collect();
            format!("{}({})", name, arg_strs.join(", "))
        }
        Expr::BinaryOp { op, left, right } => match op {
            BinOp::IsNull => format!("{} IS NULL", expr_display(left)),
            BinOp::IsNotNull => format!("{} IS NOT NULL", expr_display(left)),
            _ => {
                let op_str = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Mod => "%",
                    BinOp::Concat => "||",
                    BinOp::Eq => "=",
                    BinOp::Ne => "<>",
                    BinOp::Lt => "<",
                    BinOp::Le => "<=",
                    BinOp::Gt => ">",
                    BinOp::Ge => ">=",
                    BinOp::And => "AND",
                    BinOp::Or => "OR",
                    BinOp::IsNull | BinOp::IsNotNull => unreachable!(),
                };
                format!("{} {} {}", expr_display(left), op_str, expr_display(right))
            }
        },
        Expr::UnaryOp { op, expr } => {
            let op_str = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "NOT",
            };
            format!("{}{}", op_str, expr_display(expr))
        }
        Expr::Star => "*".to_string(),
    }
}

fn format_table(columns: &[String], rows: &[Row]) -> String {
    if rows.is_empty() && columns.is_empty() {
        return String::new();
    }

    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();

    let str_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| row.iter().map(|v| format!("{}", v)).collect())
        .collect();

    for row in &str_rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    let mut output = String::new();

    // Header
    for (i, col) in columns.iter().enumerate() {
        if i > 0 {
            output.push_str(" | ");
        }
        output.push_str(&format!("{:>width$}", col, width = widths[i]));
    }
    output.push('\n');

    // Separator
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            output.push_str("-+-");
        }
        output.push_str(&"-".repeat(*w));
    }
    output.push('\n');

    // Rows
    for row in &str_rows {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                output.push_str(" | ");
            }
            output.push_str(&format!("{:>width$}", cell, width = widths[i]));
        }
        output.push('\n');
    }

    output
}

fn format_csv(columns: &[String], rows: &[Row]) -> String {
    let mut output = String::new();
    for (i, col) in columns.iter().enumerate() {
        if i > 0 {
            output.push(',');
        }
        output.push_str(&csv_quote(col));
    }
    output.push('\n');

    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i > 0 {
                output.push(',');
            }
            output.push_str(&csv_quote(&format!("{}", val)));
        }
        output.push('\n');
    }

    output
}

fn csv_quote(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

fn format_json(columns: &[String], rows: &[Row]) -> String {
    let arr: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let map: serde_json::Map<String, serde_json::Value> = columns
                .iter()
                .zip(row.iter())
                .map(|(col, val)| {
                    let json_val = match val {
                        Value::Null => serde_json::Value::Null,
                        Value::Int(v) => serde_json::Value::Number((*v).into()),
                        Value::Real(v) => serde_json::Number::from_f64(v.0)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::Null),
                        Value::Text(s) => serde_json::Value::String(s.clone()),
                        Value::Bool(b) => serde_json::Value::Bool(*b),
                    };
                    (col.clone(), json_val)
                })
                .collect();
            serde_json::Value::Object(map)
        })
        .collect();

    serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".to_string())
}

fn bincode_serialize_columns(cols: &[ColumnDef]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&(cols.len() as u32).to_le_bytes());
    for col in cols {
        let name_bytes = col.name.as_bytes();
        buf.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(name_bytes);
        buf.push(col.col_type as u8);
        buf.push(col.primary_key as u8);
        buf.push(col.not_null as u8);
        buf.push(col.unique as u8);
    }
    buf
}

pub fn run_repl(db: &mut MiniDb) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("minidb> ");
        stdout.flush()?;

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => return Err(MiniDbError::Io(e.to_string())),
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('.') {
            match db.meta_command(trimmed) {
                Ok(Some(output)) => print!("{}", output),
                Ok(None) => break,
                Err(e) => eprintln!("Error: {}", e),
            }
            continue;
        }

        // Read until semicolon
        let mut sql = line.clone();
        while !sql.trim().ends_with(';') {
            print!("  ... ");
            stdout.flush()?;
            let mut extra = String::new();
            match stdin.lock().read_line(&mut extra) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => return Err(MiniDbError::Io(e.to_string())),
            }
            sql.push_str(&extra);
        }

        match db.execute_sql(&sql) {
            Ok(result) => {
                let output = db.format_result(&result);
                if !output.is_empty() {
                    print!("{}", output);
                }
                if !db.quiet {
                    match &result {
                        ExecResult::Rows { rows, .. } => {
                            eprintln!(
                                "ok {} {}",
                                rows.len(),
                                if rows.len() == 1 { "row" } else { "rows" }
                            );
                        }
                        ExecResult::Changed { rows_affected } => {
                            eprintln!(
                                "ok {} {}",
                                rows_affected,
                                if *rows_affected == 1 { "row" } else { "rows" }
                            );
                        }
                    }
                }
            }
            Err(e) => eprintln!("Error: {}", e),
        }
    }

    Ok(())
}

pub fn run_eval(db: &mut MiniDb, sql: &str) -> Result<()> {
    let stmts: Vec<&str> = sql.split(';').filter(|s| !s.trim().is_empty()).collect();

    for stmt_sql in &stmts {
        let trimmed = stmt_sql.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('.') {
            match db.meta_command(trimmed) {
                Ok(Some(output)) => print!("{}", output),
                Ok(None) => break,
                Err(e) => return Err(e),
            }
            continue;
        }

        match db.execute_sql(trimmed) {
            Ok(result) => {
                let output = db.format_result(&result);
                if !output.is_empty() {
                    print!("{}", output);
                }
                if !db.quiet {
                    match &result {
                        ExecResult::Rows { rows, .. } => {
                            eprintln!(
                                "ok {} {}",
                                rows.len(),
                                if rows.len() == 1 { "row" } else { "rows" }
                            );
                        }
                        ExecResult::Changed { rows_affected } => {
                            eprintln!(
                                "ok {} {}",
                                rows_affected,
                                if *rows_affected == 1 { "row" } else { "rows" }
                            );
                        }
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

pub fn run_file(db: &mut MiniDb, path: &Path, continue_on_error: bool) -> Result<()> {
    let content = fs::read_to_string(path)
        .map_err(|e| MiniDbError::Io(format!("failed to read {}: {}", path.display(), e)))?;

    let stmts: Vec<&str> = content
        .split(';')
        .filter(|s| !s.trim().is_empty())
        .collect();

    for (i, stmt_sql) in stmts.iter().enumerate() {
        let trimmed = stmt_sql.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('.') {
            match db.meta_command(trimmed) {
                Ok(Some(output)) => print!("{}", output),
                Ok(None) => break,
                Err(e) => {
                    if continue_on_error {
                        eprintln!("Error at statement {}: {}", i + 1, e);
                        continue;
                    }
                    return Err(e);
                }
            }
            continue;
        }

        match db.execute_sql(trimmed) {
            Ok(result) => {
                let output = db.format_result(&result);
                if !output.is_empty() {
                    print!("{}", output);
                }
                if !db.quiet {
                    match &result {
                        ExecResult::Rows { rows, .. } => {
                            eprintln!(
                                "ok {} {}",
                                rows.len(),
                                if rows.len() == 1 { "row" } else { "rows" }
                            );
                        }
                        ExecResult::Changed { rows_affected } => {
                            eprintln!(
                                "ok {} {}",
                                rows_affected,
                                if *rows_affected == 1 { "row" } else { "rows" }
                            );
                        }
                    }
                }
            }
            Err(e) => {
                if continue_on_error {
                    eprintln!("Error at statement {}: {}", i + 1, e);
                    continue;
                }
                return Err(e);
            }
        }
    }

    Ok(())
}

pub fn init_db(path: &Path) -> Result<()> {
    let heap_path = path.with_extension("dat");
    let wal_path = path.with_extension("wal");

    fs::File::create(&heap_path)
        .map_err(|e| MiniDbError::Io(format!("failed to create {}: {}", heap_path.display(), e)))?;
    fs::File::create(&wal_path)
        .map_err(|e| MiniDbError::Io(format!("failed to create {}: {}", wal_path.display(), e)))?;

    println!("Initialized empty database at {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_format_table() {
        let cols = vec!["id".into(), "name".into()];
        let rows = vec![
            vec![Value::Int(1), Value::Text("Alice".into())],
            vec![Value::Int(2), Value::Text("Bob".into())],
        ];
        let output = format_table(&cols, &rows);
        assert!(output.contains("id"));
        assert!(output.contains("name"));
        assert!(output.contains("Alice"));
    }

    #[test]
    fn test_format_csv() {
        let cols = vec!["id".into(), "name".into()];
        let rows = vec![vec![Value::Int(1), Value::Text("Alice".into())]];
        let output = format_csv(&cols, &rows);
        assert!(output.contains("id,name"));
        assert!(output.contains("1,Alice"));
    }

    #[test]
    fn test_format_json() {
        let cols = vec!["id".into(), "name".into()];
        let rows = vec![vec![Value::Int(1), Value::Text("Alice".into())]];
        let output = format_json(&cols, &rows);
        assert!(output.contains("\"id\": 1"));
        assert!(output.contains("\"name\": \"Alice\""));
    }

    #[test]
    fn test_csv_quote() {
        assert_eq!(csv_quote("hello"), "hello");
        assert_eq!(csv_quote("hello, world"), "\"hello, world\"");
        assert_eq!(csv_quote("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn test_mini_db_e2e() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        init_db(&db_path).unwrap();

        let mut db = MiniDb::open(&db_path, true).unwrap();

        db.execute_sql("CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);")
            .unwrap();
        db.execute_sql("INSERT INTO users (id, name) VALUES (1, 'Alice');")
            .unwrap();
        db.execute_sql("INSERT INTO users (id, name) VALUES (2, 'Bob');")
            .unwrap();

        let result = db.execute_sql("SELECT * FROM users;").unwrap();
        match result {
            ExecResult::Rows { rows, .. } => assert_eq!(rows.len(), 2),
            _ => panic!("expected rows"),
        }

        let result = db.execute_sql("SELECT * FROM users WHERE id = 1;").unwrap();
        match result {
            ExecResult::Rows { rows, .. } => assert_eq!(rows.len(), 1),
            _ => panic!("expected rows"),
        }

        db.execute_sql("UPDATE users SET name = 'Charlie' WHERE id = 1;")
            .unwrap();
        let result = db.execute_sql("SELECT * FROM users WHERE id = 1;").unwrap();
        match result {
            ExecResult::Rows { rows, .. } => {
                assert_eq!(rows[0][1], Value::Text("Charlie".into()));
            }
            _ => panic!("expected rows"),
        }

        db.execute_sql("DELETE FROM users WHERE id = 2;").unwrap();
        let result = db.execute_sql("SELECT * FROM users;").unwrap();
        match result {
            ExecResult::Rows { rows, .. } => assert_eq!(rows.len(), 1),
            _ => panic!("expected rows"),
        }
    }
}

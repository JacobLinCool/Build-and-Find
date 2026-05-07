use std::collections::BTreeMap;

use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast;
use crate::storage::{BTreeIndex, HeapFile, PageManager, Wal, WalRecord};
use crate::txn::TxnManager;
use crate::types::{ColumnDef, ColumnType, IndexDef, Row, RowId, TableSchema, Value};

/// Schema of a result set (column names and types)
#[derive(Debug, Clone)]
pub struct ResultSchema {
    pub columns: Vec<(String, ColumnType)>,
}

/// A result set from execution
#[derive(Debug)]
pub struct ResultSet {
    pub schema: ResultSchema,
    pub rows: Vec<Row>,
    pub rows_affected: u64,
}

/// The main database engine
pub struct Engine {
    pub catalog: Catalog,
    pub heap_files: BTreeMap<String, HeapFile>,
    pub indexes: BTreeMap<String, BTreeIndex>,
    pub wal: Wal,
    pub page_manager: PageManager,
    pub txn_manager: TxnManager,
    pub db_path: std::path::PathBuf,
}

impl Engine {
    pub fn open(db_path: &std::path::Path) -> Result<Self> {
        let wal_path = db_path.with_extension("wal");
        let page_manager = PageManager::open(db_path)?;
        let mut wal = Wal::open(&wal_path)?;

        // Recovery
        TxnManager::recover(&mut wal, &mut PageManager::open(db_path)?)?;

        // Load catalog
        let (tables, index_defs) = page_manager.load_schemas().unwrap_or_default();

        let mut catalog = Catalog::new();
        for schema in tables {
            catalog
                .create_table(schema)
                .map_err(|e| log::warn!("{}", e))
                .ok();
        }
        for idx in index_defs {
            catalog
                .create_index(idx)
                .map_err(|e| log::warn!("{}", e))
                .ok();
        }

        let mut heap_files = BTreeMap::new();
        let mut indexes = BTreeMap::new();

        // Open heap files and indexes for each table
        for table_name in catalog.table_names().to_vec() {
            heap_files.insert(
                table_name.to_string(),
                HeapFile::new(table_name.to_string(), db_path),
            );
        }

        for idx_def in catalog.all_indexes().values() {
            let schema = catalog.get_table(&idx_def.table)?;
            let col = schema.column_index(&idx_def.column).ok_or_else(|| {
                MiniDbError::IndexError(format!("Column {} not found", idx_def.column))
            })?;
            indexes.insert(
                idx_def.name.clone(),
                BTreeIndex::new(schema.columns[col].col_type),
            );
        }

        Ok(Engine {
            catalog,
            heap_files,
            indexes,
            wal,
            page_manager,
            txn_manager: TxnManager::new(),
            db_path: db_path.to_path_buf(),
        })
    }

    pub fn execute_statement(&mut self, stmt: ast::Statement) -> Result<ResultSet> {
        match stmt {
            ast::Statement::Begin => {
                self.txn_manager.begin(&mut self.wal)?;
                Ok(ResultSet {
                    schema: ResultSchema { columns: vec![] },
                    rows: vec![],
                    rows_affected: 0,
                })
            }
            ast::Statement::Commit => {
                self.txn_manager
                    .commit(&mut self.wal, &mut self.page_manager)?;
                self.save_catalog()?;
                Ok(ResultSet {
                    schema: ResultSchema { columns: vec![] },
                    rows: vec![],
                    rows_affected: 0,
                })
            }
            ast::Statement::Rollback => {
                self.txn_manager.rollback(&mut self.wal)?;
                Ok(ResultSet {
                    schema: ResultSchema { columns: vec![] },
                    rows: vec![],
                    rows_affected: 0,
                })
            }
            ast::Statement::CreateTable(create) => self.execute_create_table(create),
            ast::Statement::DropTable(drop) => self.execute_drop_table(drop),
            ast::Statement::CreateIndex(create) => self.execute_create_index(create),
            ast::Statement::DropIndex(drop) => self.execute_drop_index(drop),
            ast::Statement::Insert(ins) => self.execute_insert(ins),
            ast::Statement::Select(sel) => self.execute_select(sel),
            ast::Statement::Update(upd) => self.execute_update(upd),
            ast::Statement::Delete(del) => self.execute_delete(del),
        }
    }

    fn save_catalog(&mut self) -> Result<()> {
        let tables: Vec<TableSchema> = self.catalog.all_tables().values().cloned().collect();
        let indexes: Vec<IndexDef> = self.catalog.all_indexes().values().cloned().collect();
        self.page_manager.save_schema(&tables, &indexes)?;
        Ok(())
    }

    fn auto_commit(&mut self) -> Result<()> {
        // Flush all heap files
        for heap in self.heap_files.values() {
            heap.flush()?;
        }
        if !self.txn_manager.is_active() {
            // Auto-commit mode (no explicit transaction) - flush and truncate WAL
            self.page_manager.flush_all()?;
            self.wal.truncate()?;
        }
        // If a transaction is active, just flush heap files - COMMIT will handle the rest
        Ok(())
    }

    fn execute_create_table(&mut self, create: ast::CreateTableStmt) -> Result<ResultSet> {
        let mut columns = Vec::new();
        for col in create.columns {
            columns.push(ColumnDef {
                name: col.name,
                col_type: col.col_type,
                nullable: !col.primary_key && !col.not_null,
                primary_key: col.primary_key,
                unique: col.unique,
                default: col.default,
            });
        }

        let schema = TableSchema {
            name: create.table_name.clone(),
            columns,
            row_count: 0,
        };

        // Check constraints
        for constraint in &create.table_constraints {
            match constraint {
                ast::TableConstraint::Check(_) => {
                    // CHECK constraints are stored but not enforced on INSERT in this implementation
                }
            }
        }

        self.catalog.create_table(schema.clone())?;

        // Create heap file
        self.heap_files.insert(
            create.table_name.clone(),
            HeapFile::new(schema.name.clone(), &self.db_path),
        );

        self.wal.append(&WalRecord::CreateTable { schema })?;
        self.save_catalog()?;

        self.auto_commit()?;

        Ok(ResultSet {
            schema: ResultSchema { columns: vec![] },
            rows: vec![],
            rows_affected: 0,
        })
    }

    fn execute_drop_table(&mut self, drop: ast::DropTableStmt) -> Result<ResultSet> {
        // Delete heap file from disk first
        if let Some(heap) = self.heap_files.get(&drop.table_name) {
            heap.delete_file()?;
        }

        // Remove indexes for this table
        let index_names: Vec<String> = self
            .catalog
            .all_indexes()
            .values()
            .filter(|i| i.table == drop.table_name)
            .map(|i| i.name.clone())
            .collect();
        for name in &index_names {
            self.catalog.drop_index(name).ok();
            self.indexes.remove(name);
        }

        self.catalog.drop_table(&drop.table_name)?;
        self.heap_files.remove(&drop.table_name);

        self.wal.append(&WalRecord::DropTable {
            name: drop.table_name,
        })?;
        self.save_catalog()?;

        self.auto_commit()?;

        Ok(ResultSet {
            schema: ResultSchema { columns: vec![] },
            rows: vec![],
            rows_affected: 0,
        })
    }

    fn execute_create_index(&mut self, create: ast::CreateIndexStmt) -> Result<ResultSet> {
        let schema = self.catalog.get_table(&create.table_name)?;
        let col_idx = schema
            .column_index(&create.column_name)
            .ok_or_else(|| MiniDbError::ColumnNotFound(create.column_name.clone()))?;
        let col_type = schema.columns[col_idx].col_type;

        let idx_def = IndexDef {
            name: create.index_name.clone(),
            table: create.table_name.clone(),
            column: create.column_name.clone(),
        };

        self.catalog.create_index(idx_def.clone())?;
        self.indexes
            .insert(create.index_name.clone(), BTreeIndex::new(col_type));

        // Populate index with existing data
        if let Some(heap) = self.heap_files.get(&create.table_name) {
            let btree = self.indexes.get_mut(&create.index_name).unwrap();
            let rows = heap.get_all_rows();
            for (row_id, row) in rows {
                if let Some(val) = row.get(col_idx) {
                    btree.insert(val.clone(), row_id);
                }
            }
        }

        self.wal
            .append(&WalRecord::CreateIndex { index: idx_def })?;
        self.save_catalog()?;

        self.auto_commit()?;

        Ok(ResultSet {
            schema: ResultSchema { columns: vec![] },
            rows: vec![],
            rows_affected: 0,
        })
    }

    fn execute_drop_index(&mut self, drop: ast::DropIndexStmt) -> Result<ResultSet> {
        self.catalog.drop_index(&drop.index_name)?;
        self.indexes.remove(&drop.index_name);

        self.wal.append(&WalRecord::DropIndex {
            name: drop.index_name,
        })?;
        self.save_catalog()?;

        self.auto_commit()?;

        Ok(ResultSet {
            schema: ResultSchema { columns: vec![] },
            rows: vec![],
            rows_affected: 0,
        })
    }

    fn execute_insert(&mut self, ins: ast::InsertStmt) -> Result<ResultSet> {
        let schema = self.catalog.get_table(&ins.table_name)?.clone();
        let heap = self
            .heap_files
            .get_mut(&ins.table_name)
            .ok_or_else(|| MiniDbError::TableNotFound(ins.table_name.clone()))?;

        let mut total_inserted = 0u64;

        for value_row in &ins.values {
            let mut row: Row = Vec::with_capacity(schema.columns.len());

            if let Some(ref cols) = ins.columns {
                // Named columns
                for col_def in &schema.columns {
                    if let Some(pos) = cols
                        .iter()
                        .position(|c| c.eq_ignore_ascii_case(&col_def.name))
                    {
                        let val = evaluate_expr(&value_row[pos], &Row::new(), &schema)?;
                        validate_value(&val, col_def)?;
                        row.push(val);
                    } else {
                        // Use default or NULL
                        if let Some(ref default) = col_def.default {
                            row.push(default.clone());
                        } else if col_def.nullable {
                            row.push(Value::Null);
                        } else {
                            return Err(MiniDbError::ConstraintViolation(format!(
                                "Column '{}' requires a value",
                                col_def.name
                            )));
                        }
                    }
                }
            } else {
                // Positional - all columns
                if value_row.len() != schema.columns.len() {
                    return Err(MiniDbError::SqlError(format!(
                        "Expected {} values, got {}",
                        schema.columns.len(),
                        value_row.len()
                    )));
                }
                for (i, col_def) in schema.columns.iter().enumerate() {
                    let val = evaluate_expr(&value_row[i], &Row::new(), &schema)?;
                    validate_value(&val, col_def)?;
                    row.push(val);
                }
            }

            let row_id = heap.insert_row(row.clone());

            // WAL record
            self.wal.append(&WalRecord::Insert {
                table: ins.table_name.clone(),
                row_id,
                row: row.clone(),
            })?;

            // Update indexes
            for (idx_name, btree) in self.indexes.iter_mut() {
                let idx_def = self.catalog.get_index(idx_name)?;
                if idx_def.table == ins.table_name {
                    if let Some(col_pos) = schema.column_index(&idx_def.column) {
                        btree.insert(row[col_pos].clone(), row_id);
                    }
                }
            }

            total_inserted += 1;
        }

        // Update row count
        if let Ok(table) = self.catalog.get_table_mut(&ins.table_name) {
            table.row_count += total_inserted;
        }

        self.auto_commit()?;

        Ok(ResultSet {
            schema: ResultSchema { columns: vec![] },
            rows: vec![],
            rows_affected: total_inserted,
        })
    }

    fn execute_select(&mut self, sel: ast::SelectStmt) -> Result<ResultSet> {
        // For simplicity, directly execute without full planning pipeline
        self.execute_select_direct(sel)
    }

    fn execute_select_direct(&mut self, sel: ast::SelectStmt) -> Result<ResultSet> {
        // Resolve table name
        let table_name = match &sel.from {
            ast::FromClause::Table(name) => name.clone(),
            ast::FromClause::Join { left, .. } => match left.as_ref() {
                ast::FromClause::Table(name) => name.clone(),
                _ => return Err(MiniDbError::SqlError("Only simple joins supported".into())),
            },
        };

        let schema = self.catalog.get_table(&table_name)?.clone();
        let heap = self
            .heap_files
            .get(&table_name)
            .ok_or_else(|| MiniDbError::TableNotFound(table_name.clone()))?;

        // Collect all rows with row IDs
        let mut result_rows: Vec<(RowId, Row)> = heap
            .get_all_rows()
            .into_iter()
            .map(|(rid, row)| (rid, row.clone()))
            .collect();

        // Handle JOIN
        if let ast::FromClause::Join { left: _, right, on } = &sel.from {
            let right_table = match right.as_str() {
                name => name,
            };
            let right_schema = self.catalog.get_table(right_table)?.clone();
            let right_heap = self
                .heap_files
                .get(right_table)
                .ok_or_else(|| MiniDbError::TableNotFound(right_table.to_string()))?;
            let right_rows: Vec<Row> = right_heap
                .get_all_rows()
                .into_iter()
                .map(|(_, row)| row.clone())
                .collect();

            let mut joined_rows = Vec::new();
            for (left_rid, left_row) in &result_rows {
                for right_row in &right_rows {
                    let mut combined = left_row.clone();
                    combined.extend_from_slice(right_row);
                    let combined_schema = merge_schemas(&schema, &right_schema);
                    let cond = evaluate_expr(on, &combined, &combined_schema)?;
                    if cond.as_bool() == Some(true) {
                        joined_rows.push((*left_rid, combined));
                    }
                }
            }
            result_rows = joined_rows;
        }

        // Apply WHERE filter
        if let Some(ref where_e) = sel.where_clause {
            result_rows.retain(|(_, row)| {
                evaluate_expr(where_e, row, &schema)
                    .map(|v| v.as_bool() == Some(true))
                    .unwrap_or(false)
            });
        }

        // Apply ORDER BY
        if !sel.order_by.is_empty() {
            let order_by = sel.order_by.clone();
            result_rows.sort_by(|a, b| {
                for ob in &order_by {
                    let val_a = evaluate_expr(&ob.expr, &a.1, &schema).unwrap_or(Value::Null);
                    let val_b = evaluate_expr(&ob.expr, &b.1, &schema).unwrap_or(Value::Null);
                    let cmp = compare_values_exec(&val_a, &val_b);
                    if cmp != std::cmp::Ordering::Equal {
                        return if ob.ascending { cmp } else { cmp.reverse() };
                    }
                }
                std::cmp::Ordering::Equal
            });
        }

        // Apply OFFSET
        let offset = sel.offset.unwrap_or(0) as usize;
        if offset > 0 && offset < result_rows.len() {
            result_rows = result_rows[offset..].to_vec();
        } else if offset >= result_rows.len() {
            result_rows.clear();
        }

        // Apply LIMIT
        if let Some(limit) = sel.limit {
            let limit = limit as usize;
            if limit < result_rows.len() {
                result_rows.truncate(limit);
            }
        }

        // Project columns
        let mut result_schema_cols = Vec::new();
        let mut projected_rows = Vec::new();

        // Build schema and project rows
        let is_star = sel
            .columns
            .iter()
            .any(|c| matches!(c, ast::SelectColumn::Star));

        if is_star {
            for c in &schema.columns {
                result_schema_cols.push((c.name.clone(), c.col_type));
            }
            for (_, row) in &result_rows {
                projected_rows.push(row.clone());
            }
        } else {
            // Build column schema
            for col in &sel.columns {
                match col {
                    ast::SelectColumn::Expr(expr, alias) => {
                        let col_name = alias.clone().unwrap_or_else(|| format!("{:?}", expr));
                        let val = if let Some((_, row)) = result_rows.first() {
                            evaluate_expr(expr, row, &schema)?
                        } else {
                            Value::Null
                        };
                        let col_type = val.column_type().unwrap_or(ColumnType::Text);
                        result_schema_cols.push((col_name, col_type));
                    }
                    _ => {}
                }
            }
            // Project rows
            for (_, row) in &result_rows {
                let mut projected = Vec::new();
                for col in &sel.columns {
                    match col {
                        ast::SelectColumn::Expr(expr, _) => {
                            projected.push(evaluate_expr(expr, row, &schema)?);
                        }
                        _ => {}
                    }
                }
                projected_rows.push(projected);
            }
        }

        Ok(ResultSet {
            schema: ResultSchema {
                columns: result_schema_cols,
            },
            rows: projected_rows,
            rows_affected: 0,
        })
    }

    fn execute_update(&mut self, upd: ast::UpdateStmt) -> Result<ResultSet> {
        let schema = self.catalog.get_table(&upd.table_name)?.clone();
        let heap = self
            .heap_files
            .get_mut(&upd.table_name)
            .ok_or_else(|| MiniDbError::TableNotFound(upd.table_name.clone()))?;

        let all_rows: Vec<(RowId, Row)> = heap
            .get_all_rows()
            .into_iter()
            .map(|(rid, row)| (rid, row.clone()))
            .collect();

        let mut affected = 0u64;
        let mut updates: Vec<(RowId, Row, Row)> = Vec::new();

        for (row_id, mut row) in all_rows {
            // Check WHERE
            if let Some(ref where_e) = upd.where_clause {
                let cond = evaluate_expr(where_e, &row, &schema)?;
                if cond.as_bool() != Some(true) {
                    continue;
                }
            }

            let old_row = row.clone();

            // Apply assignments
            for (col_name, expr) in &upd.assignments {
                let col_idx = schema
                    .column_index(col_name)
                    .ok_or_else(|| MiniDbError::ColumnNotFound(col_name.clone()))?;
                let new_val = evaluate_expr(expr, &row, &schema)?;
                validate_value(&new_val, &schema.columns[col_idx])?;
                row[col_idx] = new_val;
            }

            updates.push((row_id, old_row, row.clone()));
            affected += 1;
        }

        // Apply updates to heap and write WAL records
        let heap = self.heap_files.get_mut(&upd.table_name).unwrap();
        for (row_id, old_row, new_row) in updates {
            self.wal.append(&WalRecord::Update {
                table: upd.table_name.clone(),
                row_id,
                old_row,
                new_row: new_row.clone(),
            })?;
            heap.update_row(row_id, new_row);
        }

        self.auto_commit()?;

        Ok(ResultSet {
            schema: ResultSchema { columns: vec![] },
            rows: vec![],
            rows_affected: affected,
        })
    }

    fn execute_delete(&mut self, del: ast::DeleteStmt) -> Result<ResultSet> {
        let schema = self.catalog.get_table(&del.table_name)?.clone();
        let heap = self
            .heap_files
            .get_mut(&del.table_name)
            .ok_or_else(|| MiniDbError::TableNotFound(del.table_name.clone()))?;

        let all_rows: Vec<(RowId, Row)> = heap
            .get_all_rows()
            .into_iter()
            .map(|(rid, row)| (rid, row.clone()))
            .collect();

        let mut affected = 0u64;
        let mut to_delete: Vec<(RowId, Row)> = Vec::new();

        for (row_id, row) in all_rows {
            if let Some(ref where_e) = del.where_clause {
                let cond = evaluate_expr(where_e, &row, &schema)?;
                if cond.as_bool() != Some(true) {
                    continue;
                }
            }
            to_delete.push((row_id, row));
            affected += 1;
        }

        // Remove from heap, indexes, and write WAL
        let heap = self.heap_files.get_mut(&del.table_name).unwrap();
        for (row_id, row) in to_delete {
            self.wal.append(&WalRecord::Delete {
                table: del.table_name.clone(),
                row_id,
            })?;

            // Remove from indexes
            for (idx_name, btree) in self.indexes.iter_mut() {
                let idx_def = self.catalog.get_index(idx_name)?;
                if idx_def.table == del.table_name {
                    if let Some(col_pos) = schema.column_index(&idx_def.column) {
                        btree.delete(&row[col_pos], row_id);
                    }
                }
            }

            heap.delete_row(row_id);
        }

        if let Ok(table) = self.catalog.get_table_mut(&del.table_name) {
            table.row_count = table.row_count.saturating_sub(affected);
        }

        self.auto_commit()?;

        Ok(ResultSet {
            schema: ResultSchema { columns: vec![] },
            rows: vec![],
            rows_affected: affected,
        })
    }
}

fn validate_value(val: &Value, col_def: &ColumnDef) -> Result<()> {
    if val.is_null() && !col_def.nullable {
        return Err(MiniDbError::ConstraintViolation(format!(
            "Column '{}' does not allow NULL",
            col_def.name
        )));
    }
    // Type coercion
    match col_def.col_type {
        ColumnType::Int => {
            if !val.is_null() && val.as_int().is_none() {
                return Err(MiniDbError::TypeError(format!(
                    "Cannot convert {} to INT",
                    val
                )));
            }
        }
        ColumnType::Real => {
            if !val.is_null() && val.as_real().is_none() {
                return Err(MiniDbError::TypeError(format!(
                    "Cannot convert {} to REAL",
                    val
                )));
            }
        }
        ColumnType::Text => {
            // Most values can be converted to text
        }
        ColumnType::Bool => {
            if !val.is_null() && val.as_bool().is_none() {
                return Err(MiniDbError::TypeError(format!(
                    "Cannot convert {} to BOOL",
                    val
                )));
            }
        }
    }
    Ok(())
}

fn merge_schemas(a: &TableSchema, b: &TableSchema) -> TableSchema {
    let mut columns = a.columns.clone();
    columns.extend(b.columns.clone());
    TableSchema {
        name: format!("{}_{}", a.name, b.name),
        columns,
        row_count: 0,
    }
}

/// Evaluate an expression against a row
pub fn evaluate_expr(expr: &ast::Expr, row: &Row, schema: &TableSchema) -> Result<Value> {
    match expr {
        ast::Expr::Literal(v) => Ok(v.clone()),
        ast::Expr::Column(name) => {
            let idx = schema
                .column_index(name)
                .ok_or_else(|| MiniDbError::ColumnNotFound(name.clone()))?;
            Ok(row.get(idx).cloned().unwrap_or(Value::Null))
        }
        ast::Expr::QualifiedColumn { table, column } => {
            // For simple case, just look up by column name
            let idx = schema
                .column_index(column)
                .ok_or_else(|| MiniDbError::ColumnNotFound(format!("{}.{}", table, column)))?;
            Ok(row.get(idx).cloned().unwrap_or(Value::Null))
        }
        ast::Expr::BinaryOp { left, op, right } => {
            let l = evaluate_expr(left, row, schema)?;
            let r = evaluate_expr(right, row, schema)?;
            evaluate_binary_op(&l, op, &r)
        }
        ast::Expr::UnaryOp { op, expr } => {
            let val = evaluate_expr(expr, row, schema)?;
            match op {
                ast::UnOp::Neg => match val {
                    Value::Int(v) => Ok(Value::Int(-v)),
                    Value::Real(v) => Ok(Value::Real(-v)),
                    Value::Null => Ok(Value::Null),
                    _ => Err(MiniDbError::TypeError(
                        "Cannot negate non-numeric value".into(),
                    )),
                },
                ast::UnOp::Not => Ok(Value::not(&val)),
            }
        }
        ast::Expr::IsNull { expr, negated } => {
            let val = evaluate_expr(expr, row, schema)?;
            let is_null = val.is_null();
            Ok(Value::Bool(if *negated { !is_null } else { is_null }))
        }
        ast::Expr::Function { name, args } => {
            let mut arg_vals = Vec::new();
            for arg in args {
                arg_vals.push(evaluate_expr(arg, row, schema)?);
            }
            evaluate_function(name, &arg_vals)
        }
    }
}

fn evaluate_binary_op(left: &Value, op: &ast::BinOp, right: &Value) -> Result<Value> {
    match op {
        ast::BinOp::Add => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            if let (Some(a), Some(b)) = (left.as_int(), right.as_int()) {
                return Ok(Value::Int(a + b));
            }
            if let (Some(a), Some(b)) = (left.as_real(), right.as_real()) {
                return Ok(Value::Real(a + b));
            }
            Err(MiniDbError::TypeError("Cannot add these types".into()))
        }
        ast::BinOp::Sub => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            if let (Some(a), Some(b)) = (left.as_int(), right.as_int()) {
                return Ok(Value::Int(a - b));
            }
            if let (Some(a), Some(b)) = (left.as_real(), right.as_real()) {
                return Ok(Value::Real(a - b));
            }
            Err(MiniDbError::TypeError("Cannot subtract these types".into()))
        }
        ast::BinOp::Mul => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            if let (Some(a), Some(b)) = (left.as_int(), right.as_int()) {
                return Ok(Value::Int(a * b));
            }
            if let (Some(a), Some(b)) = (left.as_real(), right.as_real()) {
                return Ok(Value::Real(a * b));
            }
            Err(MiniDbError::TypeError("Cannot multiply these types".into()))
        }
        ast::BinOp::Div => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            if let (Some(a), Some(b)) = (left.as_int(), right.as_int()) {
                if b == 0 {
                    return Err(MiniDbError::DivisionByZero);
                }
                return Ok(Value::Int(a / b));
            }
            if let (Some(a), Some(b)) = (left.as_real(), right.as_real()) {
                if b == 0.0 {
                    return Err(MiniDbError::DivisionByZero);
                }
                return Ok(Value::Real(a / b));
            }
            Err(MiniDbError::TypeError("Cannot divide these types".into()))
        }
        ast::BinOp::Concat => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            let l = format!("{}", left);
            let r = format!("{}", right);
            Ok(Value::Text(format!("{}{}", l, r)))
        }
        ast::BinOp::Eq => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            Ok(Value::Bool(values_equal(left, right)))
        }
        ast::BinOp::Ne => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            Ok(Value::Bool(!values_equal(left, right)))
        }
        ast::BinOp::Lt => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            Ok(Value::Bool(
                compare_values_exec(left, right) == std::cmp::Ordering::Less,
            ))
        }
        ast::BinOp::Le => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            let cmp = compare_values_exec(left, right);
            Ok(Value::Bool(
                cmp == std::cmp::Ordering::Less || cmp == std::cmp::Ordering::Equal,
            ))
        }
        ast::BinOp::Gt => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            Ok(Value::Bool(
                compare_values_exec(left, right) == std::cmp::Ordering::Greater,
            ))
        }
        ast::BinOp::Ge => {
            if left.is_null() || right.is_null() {
                return Ok(Value::Null);
            }
            let cmp = compare_values_exec(left, right);
            Ok(Value::Bool(
                cmp == std::cmp::Ordering::Greater || cmp == std::cmp::Ordering::Equal,
            ))
        }
        ast::BinOp::And => Ok(Value::and(left, right)),
        ast::BinOp::Or => Ok(Value::or(left, right)),
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Int(a), Value::Real(b)) => (*a as f64) == *b,
        (Value::Real(a), Value::Int(b)) => *a == (*b as f64),
        (Value::Real(a), Value::Real(b)) => a == b,
        (Value::Text(a), Value::Text(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        _ => false,
    }
}

fn compare_values_exec(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
        (Value::Null, _) => std::cmp::Ordering::Less,
        (_, Value::Null) => std::cmp::Ordering::Greater,
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Int(a), Value::Real(b)) => (*a as f64)
            .partial_cmp(b)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Value::Real(a), Value::Int(b)) => a
            .partial_cmp(&(*b as f64))
            .unwrap_or(std::cmp::Ordering::Equal),
        (Value::Real(a), Value::Real(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Text(a), Value::Text(b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        _ => std::cmp::Ordering::Equal,
    }
}

fn evaluate_function(name: &str, args: &[Value]) -> Result<Value> {
    match name {
        "LENGTH" => {
            if args.len() != 1 {
                return Err(MiniDbError::RuntimeError(
                    "LENGTH requires 1 argument".into(),
                ));
            }
            match &args[0] {
                Value::Text(s) => Ok(Value::Int(s.len() as i64)),
                Value::Null => Ok(Value::Null),
                _ => Ok(Value::Int(format!("{}", args[0]).len() as i64)),
            }
        }
        "LOWER" => {
            if args.len() != 1 {
                return Err(MiniDbError::RuntimeError(
                    "LOWER requires 1 argument".into(),
                ));
            }
            match &args[0] {
                Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
                Value::Null => Ok(Value::Null),
                _ => Ok(Value::Text(format!("{}", args[0]).to_lowercase())),
            }
        }
        "UPPER" => {
            if args.len() != 1 {
                return Err(MiniDbError::RuntimeError(
                    "UPPER requires 1 argument".into(),
                ));
            }
            match &args[0] {
                Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
                Value::Null => Ok(Value::Null),
                _ => Ok(Value::Text(format!("{}", args[0]).to_uppercase())),
            }
        }
        "SUBSTR" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(MiniDbError::RuntimeError(
                    "SUBSTR requires 2-3 arguments".into(),
                ));
            }
            let s = match &args[0] {
                Value::Text(s) => s.clone(),
                Value::Null => return Ok(Value::Null),
                _ => format!("{}", args[0]),
            };
            let start = match args[1].as_int() {
                Some(v) => (v - 1).max(0) as usize,
                None => return Ok(Value::Null),
            };
            let len = if args.len() > 2 {
                args[2].as_int().unwrap_or(s.len() as i64) as usize
            } else {
                s.len()
            };
            let end = (start + len).min(s.len());
            if start >= s.len() {
                Ok(Value::Text(String::new()))
            } else {
                Ok(Value::Text(s[start..end].to_string()))
            }
        }
        "ABS" => {
            if args.len() != 1 {
                return Err(MiniDbError::RuntimeError("ABS requires 1 argument".into()));
            }
            match &args[0] {
                Value::Int(v) => Ok(Value::Int(v.abs())),
                Value::Real(v) => Ok(Value::Real(v.abs())),
                Value::Null => Ok(Value::Null),
                _ => Err(MiniDbError::TypeError(
                    "ABS requires numeric argument".into(),
                )),
            }
        }
        "COALESCE" => {
            for arg in args {
                if !arg.is_null() {
                    return Ok(arg.clone());
                }
            }
            Ok(Value::Null)
        }
        _ => Err(MiniDbError::RuntimeError(format!(
            "Unknown function: {}",
            name
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use std::env;

    fn make_test_engine(dir: &Path) -> Engine {
        let db_path = dir.join("test.db");
        Engine::open(&db_path).unwrap()
    }

    fn execute_sql(engine: &mut Engine, sql: &str) -> Result<ResultSet> {
        let tokens = tokenize(sql).map_err(|(msg, _, _)| MiniDbError::ParseError {
            message: msg,
            line: 0,
            column: 0,
        })?;
        let stmts = parse(tokens)?;
        engine.execute_statement(stmts.into_iter().next().unwrap())
    }

    use std::path::Path;

    #[test]
    fn test_create_table_and_insert() {
        let dir = env::temp_dir().join("minidb_test_exec1");
        std::fs::create_dir_all(&dir).unwrap();
        let mut engine = make_test_engine(&dir);

        execute_sql(
            &mut engine,
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT);",
        )
        .unwrap();
        execute_sql(&mut engine, "INSERT INTO t (id, name) VALUES (1, 'Alice');").unwrap();
        let rs = execute_sql(&mut engine, "SELECT * FROM t;").unwrap();
        assert_eq!(rs.rows.len(), 1);
        assert_eq!(rs.rows[0][0], Value::Int(1));
        assert_eq!(rs.rows[0][1], Value::Text("Alice".into()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_select_where() {
        let dir = env::temp_dir().join("minidb_test_exec2");
        std::fs::create_dir_all(&dir).unwrap();
        let mut engine = make_test_engine(&dir);

        execute_sql(&mut engine, "CREATE TABLE t (id INT, val TEXT);").unwrap();
        execute_sql(&mut engine, "INSERT INTO t VALUES (1, 'a');").unwrap();
        execute_sql(&mut engine, "INSERT INTO t VALUES (2, 'b');").unwrap();
        execute_sql(&mut engine, "INSERT INTO t VALUES (3, 'c');").unwrap();

        let rs = execute_sql(&mut engine, "SELECT * FROM t WHERE id = 2;").unwrap();
        assert_eq!(rs.rows.len(), 1);
        assert_eq!(rs.rows[0][0], Value::Int(2));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_update() {
        let dir = env::temp_dir().join("minidb_test_exec3");
        std::fs::create_dir_all(&dir).unwrap();
        let mut engine = make_test_engine(&dir);

        execute_sql(&mut engine, "CREATE TABLE t (id INT, name TEXT);").unwrap();
        execute_sql(&mut engine, "INSERT INTO t VALUES (1, 'old');").unwrap();
        let rs = execute_sql(&mut engine, "UPDATE t SET name = 'new' WHERE id = 1;").unwrap();
        assert_eq!(rs.rows_affected, 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_delete() {
        let dir = env::temp_dir().join("minidb_test_exec4");
        std::fs::create_dir_all(&dir).unwrap();
        let mut engine = make_test_engine(&dir);

        execute_sql(&mut engine, "CREATE TABLE t (id INT);").unwrap();
        execute_sql(&mut engine, "INSERT INTO t VALUES (1);").unwrap();
        execute_sql(&mut engine, "INSERT INTO t VALUES (2);").unwrap();
        let rs = execute_sql(&mut engine, "DELETE FROM t WHERE id = 1;").unwrap();
        assert_eq!(rs.rows_affected, 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_evaluate_binary_op() {
        let l = Value::Int(3);
        let r = Value::Int(4);
        assert_eq!(
            evaluate_binary_op(&l, &ast::BinOp::Add, &r).unwrap(),
            Value::Int(7)
        );
        assert_eq!(
            evaluate_binary_op(&l, &ast::BinOp::Mul, &r).unwrap(),
            Value::Int(12)
        );
        assert_eq!(
            evaluate_binary_op(&l, &ast::BinOp::Lt, &r).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn test_evaluate_function() {
        assert_eq!(
            evaluate_function("LENGTH", &[Value::Text("hello".into())]).unwrap(),
            Value::Int(5)
        );
        assert_eq!(
            evaluate_function("LOWER", &[Value::Text("HELLO".into())]).unwrap(),
            Value::Text("hello".into())
        );
        assert_eq!(
            evaluate_function("UPPER", &[Value::Text("hello".into())]).unwrap(),
            Value::Text("HELLO".into())
        );
        assert_eq!(
            evaluate_function("ABS", &[Value::Int(-5)]).unwrap(),
            Value::Int(5)
        );
        assert_eq!(
            evaluate_function("COALESCE", &[Value::Null, Value::Int(42)]).unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn test_null_propagation() {
        let l = Value::Null;
        let r = Value::Int(5);
        assert!(evaluate_binary_op(&l, &ast::BinOp::Add, &r)
            .unwrap()
            .is_null());
        assert!(evaluate_binary_op(&l, &ast::BinOp::Eq, &r)
            .unwrap()
            .is_null());
    }
}

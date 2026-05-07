#[allow(dead_code)]
pub mod operators;

use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::plan::LogicalPlan;
use crate::storage::btree::BTreeIndex;
use crate::storage::heap::HeapFile;
use crate::storage::Storage;
use crate::txn::TransactionManager;
use crate::types::*;
use std::collections::HashMap;

pub struct Engine {
    pub storage: Storage,
    pub catalog: Catalog,
    pub txn: TransactionManager,
    current_table_map: HashMap<String, usize>,
}

impl Engine {
    pub fn new(storage: Storage, catalog: Catalog) -> Self {
        Self {
            storage,
            catalog,
            txn: TransactionManager::new(),
            current_table_map: HashMap::new(),
        }
    }

    pub fn execute_statement(&mut self, stmt: Statement) -> Result<ExecResult> {
        match &stmt {
            Statement::Begin => {
                self.txn.begin()?;
                return Ok(ExecResult::Message("ok".into()));
            }
            Statement::Commit => {
                self.txn.commit(&mut self.storage)?;
                return Ok(ExecResult::Message("ok".into()));
            }
            Statement::Rollback => {
                self.txn.rollback(&mut self.storage)?;
                return Ok(ExecResult::Message("ok".into()));
            }
            Statement::CreateTable {
                name,
                columns,
                constraints,
            } => {
                let check_exprs: Vec<String> = constraints
                    .iter()
                    .filter_map(|c| match c {
                        TableConstraint::Check(expr) => Some(format!("{:?}", expr)),
                    })
                    .collect();
                let col_defs: Vec<crate::types::ColumnDef> = columns
                    .iter()
                    .map(|c| crate::types::ColumnDef {
                        name: c.name.clone(),
                        col_type: c.col_type,
                        primary_key: c.primary_key,
                        not_null: c.not_null,
                        unique: c.unique,
                        default: c.default.clone(),
                    })
                    .collect();
                self.catalog.create_table(
                    &mut self.storage.pm,
                    name.clone(),
                    col_defs,
                    check_exprs,
                )?;
                self.catalog.save(&mut self.storage.pm)?;
                return Ok(ExecResult::Message("ok".into()));
            }
            Statement::DropTable { name } => {
                self.catalog.drop_table(&mut self.storage.pm, name)?;
                self.catalog.save(&mut self.storage.pm)?;
                return Ok(ExecResult::Message("ok".into()));
            }
            Statement::CreateIndex {
                index_name,
                table_name,
                column_name,
            } => {
                self.catalog.create_index(
                    &mut self.storage.pm,
                    index_name.clone(),
                    table_name,
                    column_name,
                )?;
                self.catalog.save(&mut self.storage.pm)?;
                return Ok(ExecResult::Message("ok".into()));
            }
            Statement::DropIndex { name } => {
                self.catalog.drop_index(name)?;
                self.catalog.save(&mut self.storage.pm)?;
                return Ok(ExecResult::Message("ok".into()));
            }
            Statement::Meta(cmd) => {
                return self.execute_meta(cmd);
            }
            _ => {}
        }

        // For DML statements, plan and execute
        let planner = crate::plan::Planner::new(&self.catalog);
        let plan = planner.plan(&stmt)?;
        self.execute_plan(plan, &stmt)
    }

    fn execute_plan(&mut self, plan: LogicalPlan, stmt: &Statement) -> Result<ExecResult> {
        match plan {
            LogicalPlan::Insert {
                table,
                columns,
                values,
            } => self.execute_insert(&table, columns.as_deref(), &values),
            LogicalPlan::Update {
                table,
                assignments,
                filter,
            } => self.execute_update(&table, &assignments, filter.as_ref()),
            LogicalPlan::Delete { table, filter } => self.execute_delete(&table, filter.as_ref()),
            LogicalPlan::Project { input, columns } => {
                let rows = self.execute_query(*input)?;
                // Apply projection to results
                if columns.is_empty() {
                    return Ok(ExecResult::Rows {
                        columns: vec![],
                        rows: vec![],
                    });
                }
                // For now, if it's simple column references, return as-is
                // Complex projections need expression evaluation
                let first_row = rows.first();
                let col_names: Vec<String> = columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        c.alias.clone().unwrap_or_else(|| match &c.expr {
                            Expr::Column { name, .. } => name.clone(),
                            Expr::Function { name, .. } => name.clone(),
                            _ => format!("col_{}", i),
                        })
                    })
                    .collect();

                let table_map = self.current_table_map.clone();
                let mut result_rows = Vec::new();
                for row in &rows {
                    let mut new_row = Vec::new();
                    for col in &columns {
                        new_row.push(self.evaluate_expr(&col.expr, row, &table_map)?);
                    }
                    result_rows.push(new_row);
                }

                Ok(ExecResult::Rows {
                    columns: col_names,
                    rows: result_rows,
                })
            }
            _ => Err(MiniDbError::Runtime {
                message: "unsupported plan type for direct execution".into(),
            }),
        }
    }

    fn execute_query(&mut self, plan: LogicalPlan) -> Result<Vec<Row>> {
        match plan {
            LogicalPlan::SeqScan {
                table,
                alias,
                filter,
            } => {
                if table.is_empty() {
                    return Ok(vec![vec![]]);
                }
                // Set table_map for single table
                self.current_table_map.clear();
                self.current_table_map.insert(table.clone(), 0);

                let table_meta = self.catalog.get_table(&table)?.clone();
                let heap = HeapFile::new(table_meta.id, table_meta.heap_page);
                let mut rows = heap.scan_all(&mut self.storage.pm)?;
                let table_map = self.current_table_map.clone();
                let result: Vec<Row> = rows
                    .drain(..)
                    .map(|(_, _, row)| row)
                    .filter(|row| {
                        if let Some(ref pred) = filter {
                            self.evaluate_expr(pred, row, &table_map)
                                .map(|v| v == Value::Bool(true))
                                .unwrap_or(false)
                        } else {
                            true
                        }
                    })
                    .collect();
                Ok(result)
            }
            LogicalPlan::IndexScan {
                table,
                alias,
                index_name,
                key_expr,
            } => {
                self.current_table_map.clear();
                self.current_table_map.insert(table.clone(), 0);

                let table_meta = self.catalog.get_table(&table)?.clone();
                let index_meta =
                    self.catalog
                        .indexes
                        .get(&index_name)
                        .cloned()
                        .ok_or_else(|| MiniDbError::Catalog {
                            message: format!("index '{}' not found", index_name),
                        })?;

                let key_val = self.evaluate_expr(&key_expr, &[], &HashMap::new())?;
                let bt = BTreeIndex::new(index_meta.id, index_meta.root_page);

                let entries = bt.search_equal(&mut self.storage.pm, &key_val)?;
                let heap = HeapFile::new(table_meta.id, table_meta.heap_page);
                let all_rows = heap.scan_all(&mut self.storage.pm)?;

                let mut result = Vec::new();
                for entry in &entries {
                    for (pid, slot, row) in &all_rows {
                        if *pid == entry.row_page && *slot == entry.row_slot as usize {
                            result.push(row.clone());
                            break;
                        }
                    }
                }
                Ok(result)
            }
            LogicalPlan::Filter { input, predicate } => {
                let rows = self.execute_query(*input)?;
                let table_map = self.current_table_map.clone();
                Ok(rows
                    .into_iter()
                    .filter(|row| {
                        self.evaluate_expr(&predicate, row, &table_map)
                            .map(|v| v == Value::Bool(true))
                            .unwrap_or(false)
                    })
                    .collect())
            }
            LogicalPlan::Project { input, columns } => {
                let rows = self.execute_query(*input)?;
                let table_map = self.current_table_map.clone();
                let mut result = Vec::new();
                for row in &rows {
                    let mut new_row = Vec::new();
                    for col in &columns {
                        new_row.push(self.evaluate_expr(&col.expr, row, &table_map)?);
                    }
                    result.push(new_row);
                }
                Ok(result)
            }
            LogicalPlan::NestedLoopJoin { left, right, on } => {
                // Build table_map from plan structure for column resolution
                let mut table_map = HashMap::new();
                let left_offset = self.collect_table_offsets(&left, &mut table_map, 0);
                self.collect_table_offsets(&right, &mut table_map, left_offset);

                let left_rows = self.execute_query(*left)?;
                let right_rows = self.execute_query(*right)?;

                // Restore the join table_map (overwritten by recursive calls)
                self.current_table_map = table_map.clone();

                let mut result = Vec::new();

                for lrow in &left_rows {
                    for rrow in &right_rows {
                        let mut joined = lrow.clone();
                        joined.extend(rrow.iter().cloned());

                        if let Some(ref predicate) = on {
                            if self.evaluate_expr(predicate, &joined, &table_map)?
                                != Value::Bool(true)
                            {
                                continue;
                            }
                        }
                        result.push(joined);
                    }
                }
                Ok(result)
            }
            LogicalPlan::Sort { input, keys } => {
                let mut rows = self.execute_query(*input)?;
                let table_map = self.current_table_map.clone();
                rows.sort_by(|a, b| {
                    for key in &keys {
                        let va = self
                            .evaluate_expr(&key.expr, a, &table_map)
                            .unwrap_or(Value::Null);
                        let vb = self
                            .evaluate_expr(&key.expr, b, &table_map)
                            .unwrap_or(Value::Null);
                        let ord = va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal);
                        if ord != std::cmp::Ordering::Equal {
                            return if key.asc { ord } else { ord.reverse() };
                        }
                    }
                    std::cmp::Ordering::Equal
                });
                Ok(rows)
            }
            LogicalPlan::Limit {
                input,
                count,
                offset,
            } => {
                let rows = self.execute_query(*input)?;
                Ok(rows.into_iter().skip(offset).take(count).collect())
            }
            _ => Err(MiniDbError::Runtime {
                message: "unsupported query plan".into(),
            }),
        }
    }

    fn collect_table_offsets(
        &self,
        plan: &LogicalPlan,
        map: &mut HashMap<String, usize>,
        start_offset: usize,
    ) -> usize {
        match plan {
            LogicalPlan::SeqScan { table, .. } => {
                if !table.is_empty() {
                    if let Ok(meta) = self.catalog.get_table(table) {
                        let col_count = meta.columns.len();
                        map.insert(table.clone(), start_offset);
                        return col_count;
                    }
                }
                0
            }
            LogicalPlan::IndexScan { table, .. } => {
                if let Ok(meta) = self.catalog.get_table(table) {
                    let col_count = meta.columns.len();
                    map.insert(table.clone(), start_offset);
                    col_count
                } else {
                    0
                }
            }
            LogicalPlan::Filter { input, .. } => {
                self.collect_table_offsets(input, map, start_offset)
            }
            LogicalPlan::Project { input, .. } => {
                self.collect_table_offsets(input, map, start_offset)
            }
            LogicalPlan::NestedLoopJoin { left, right, .. } => {
                let left_size = self.collect_table_offsets(left, map, start_offset);
                self.collect_table_offsets(right, map, start_offset + left_size);
                left_size
            }
            _ => 0,
        }
    }

    fn execute_insert(
        &mut self,
        table_name: &str,
        columns: Option<&[String]>,
        values: &[Vec<Expr>],
    ) -> Result<ExecResult> {
        let table_meta = self.catalog.get_table(table_name)?.clone();
        let heap = HeapFile::new(table_meta.id, table_meta.heap_page);
        let txn_id = self.txn.auto_txn_id();
        let mut count = 0;

        for value_row in values {
            let mut row: Vec<Value> = if let Some(cols) = columns {
                // Named columns: map to full row
                let mut full_row = vec![Value::Null; table_meta.columns.len()];
                for (i, col_name) in cols.iter().enumerate() {
                    let col_idx = table_meta
                        .columns
                        .iter()
                        .position(|c| &c.name == col_name)
                        .ok_or_else(|| MiniDbError::Catalog {
                            message: format!("column '{}' not found", col_name),
                        })?;
                    let val = if i < value_row.len() {
                        self.evaluate_expr(&value_row[i], &[], &HashMap::new())?
                    } else {
                        Value::Null
                    };
                    full_row[col_idx] = val;
                }
                full_row
            } else {
                // Positional
                let mut r = Vec::new();
                for expr in value_row {
                    r.push(self.evaluate_expr(expr, &[], &HashMap::new())?);
                }
                r
            };

            // Apply defaults
            self.catalog.apply_defaults(&table_meta, &mut row);

            // Cast values to column types
            for (i, col) in table_meta.columns.iter().enumerate() {
                if i < row.len() && !row[i].is_null() {
                    row[i] = row[i].cast_to(col.col_type)?;
                }
            }

            // Validate
            self.catalog.validate_row(&table_meta, &row, false)?;

            // Check unique constraints
            self.catalog.check_unique_constraints(
                &mut self.storage.pm,
                &table_meta,
                &row,
                None,
                None,
            )?;

            // Insert
            let (page_id, slot) = heap.insert_row(&mut self.storage.pm, &row)?;

            // Update indexes
            for idx_meta in self.catalog.indexes.values() {
                if idx_meta.table_id == table_meta.id {
                    let bt = BTreeIndex::new(idx_meta.id, idx_meta.root_page);
                    if idx_meta.column_index < row.len() {
                        bt.insert(
                            &mut self.storage.pm,
                            &row[idx_meta.column_index],
                            page_id,
                            slot as u32,
                        )?;
                    }
                }
            }

            // WAL
            let data = crate::storage::wal::encode_row_data(table_meta.id, slot, &row);
            self.storage
                .wal
                .append(txn_id, crate::storage::wal::WAL_INSERT, &data)?;

            count += 1;
        }

        self.txn.auto_commit(&mut self.storage)?;
        Ok(ExecResult::RowsAffected(count))
    }

    fn execute_update(
        &mut self,
        table_name: &str,
        assignments: &[(String, Expr)],
        filter: Option<&Expr>,
    ) -> Result<ExecResult> {
        let table_meta = self.catalog.get_table(table_name)?.clone();
        let heap = HeapFile::new(table_meta.id, table_meta.heap_page);
        let txn_id = self.txn.auto_txn_id();
        let mut all_rows = heap.scan_all(&mut self.storage.pm)?;
        let mut count = 0;

        for (pid, slot, row) in &mut all_rows {
            // Check filter
            if let Some(pred) = filter {
                let val = self.evaluate_expr(pred, row, &HashMap::new())?;
                if val != Value::Bool(true) {
                    continue;
                }
            }

            // Apply assignments
            let mut new_row = row.clone();
            for (col_name, expr) in assignments {
                let col_idx = table_meta
                    .columns
                    .iter()
                    .position(|c| &c.name == col_name)
                    .ok_or_else(|| MiniDbError::Catalog {
                        message: format!("column '{}' not found", col_name),
                    })?;
                let val = self.evaluate_expr(expr, row, &HashMap::new())?;
                if col_idx < new_row.len() {
                    new_row[col_idx] = val.cast_to(table_meta.columns[col_idx].col_type)?;
                }
            }

            // Validate
            self.catalog.validate_row(&table_meta, &new_row, true)?;
            self.catalog.check_unique_constraints(
                &mut self.storage.pm,
                &table_meta,
                &new_row,
                Some(*pid),
                Some(*slot),
            )?;

            // Update heap
            heap.update_row(&mut self.storage.pm, *pid, *slot, &new_row)?;

            // Update indexes
            for idx_meta in self.catalog.indexes.values() {
                if idx_meta.table_id == table_meta.id {
                    let bt = BTreeIndex::new(idx_meta.id, idx_meta.root_page);
                    if idx_meta.column_index < row.len() {
                        // Delete old index entry
                        let _ = bt.delete(
                            &mut self.storage.pm,
                            &row[idx_meta.column_index],
                            *pid,
                            *slot as u32,
                        );
                    }
                    if idx_meta.column_index < new_row.len() {
                        // Insert new index entry
                        let _ = bt.insert(
                            &mut self.storage.pm,
                            &new_row[idx_meta.column_index],
                            *pid,
                            *slot as u32,
                        );
                    }
                }
            }

            // WAL
            let data = crate::storage::wal::encode_row_data(table_meta.id, *slot as u64, &new_row);
            self.storage
                .wal
                .append(txn_id, crate::storage::wal::WAL_UPDATE, &data)?;

            count += 1;
        }

        self.txn.auto_commit(&mut self.storage)?;
        Ok(ExecResult::RowsAffected(count))
    }

    fn execute_delete(&mut self, table_name: &str, filter: Option<&Expr>) -> Result<ExecResult> {
        let table_meta = self.catalog.get_table(table_name)?.clone();
        let heap = HeapFile::new(table_meta.id, table_meta.heap_page);
        let txn_id = self.txn.auto_txn_id();
        let all_rows = heap.scan_all(&mut self.storage.pm)?;
        let mut count = 0;

        for (pid, slot, row) in &all_rows {
            // Check filter
            if let Some(pred) = filter {
                let val = self.evaluate_expr(pred, row, &HashMap::new())?;
                if val != Value::Bool(true) {
                    continue;
                }
            }

            // Delete from heap
            heap.delete_row(&mut self.storage.pm, *pid, *slot)?;

            // Delete from indexes
            for idx_meta in self.catalog.indexes.values() {
                if idx_meta.table_id == table_meta.id {
                    let bt = BTreeIndex::new(idx_meta.id, idx_meta.root_page);
                    if idx_meta.column_index < row.len() {
                        let _ = bt.delete(
                            &mut self.storage.pm,
                            &row[idx_meta.column_index],
                            *pid,
                            *slot as u32,
                        );
                    }
                }
            }

            // WAL
            let data = crate::storage::wal::encode_row_data(table_meta.id, *slot as u64, &row);
            self.storage
                .wal
                .append(txn_id, crate::storage::wal::WAL_DELETE, &data)?;

            count += 1;
        }

        self.txn.auto_commit(&mut self.storage)?;
        Ok(ExecResult::RowsAffected(count))
    }

    fn execute_meta(&mut self, cmd: &MetaCommand) -> Result<ExecResult> {
        match cmd {
            MetaCommand::Schema => {
                let mut lines = Vec::new();
                for (name, table) in &self.catalog.tables {
                    let col_strs: Vec<String> = table
                        .columns
                        .iter()
                        .map(|c| {
                            let mut parts = vec![format!("{} {}", c.name, c.col_type)];
                            if c.primary_key {
                                parts.push("PRIMARY KEY".into());
                            }
                            if c.not_null && !c.primary_key {
                                parts.push("NOT NULL".into());
                            }
                            if c.unique && !c.primary_key {
                                parts.push("UNIQUE".into());
                            }
                            if let Some(ref d) = c.default {
                                parts.push(format!("DEFAULT {}", d));
                            }
                            parts.join(" ")
                        })
                        .collect();
                    lines.push(format!("CREATE TABLE {} ({});", name, col_strs.join(", ")));
                }
                for (name, idx) in &self.catalog.indexes {
                    lines.push(format!(
                        "CREATE INDEX {} ON {} ({});",
                        name,
                        self.catalog
                            .tables
                            .values()
                            .find(|t| t.id == idx.table_id)
                            .map(|t| t.name.as_str())
                            .unwrap_or("?"),
                        idx.column_name
                    ));
                }
                lines.sort();
                Ok(ExecResult::Message(lines.join("\n")))
            }
            MetaCommand::Tables => {
                let mut names: Vec<&str> = self.catalog.tables.keys().map(|s| s.as_str()).collect();
                names.sort();
                Ok(ExecResult::Message(names.join("\n")))
            }
            MetaCommand::Indexes => {
                let mut lines = Vec::new();
                for (name, idx) in &self.catalog.indexes {
                    let table_name = self
                        .catalog
                        .tables
                        .values()
                        .find(|t| t.id == idx.table_id)
                        .map(|t| t.name.as_str())
                        .unwrap_or("?");
                    lines.push(format!("{} ON {} ({})", name, table_name, idx.column_name));
                }
                lines.sort();
                Ok(ExecResult::Message(lines.join("\n")))
            }
            MetaCommand::Exit => Ok(ExecResult::Exit),
            MetaCommand::Format(kind) => Ok(ExecResult::SetFormat(kind.clone())),
            MetaCommand::Help => Ok(ExecResult::Message(
                "Available commands:\n\
                 .schema    - Show all table and index definitions\n\
                 .tables    - List all tables\n\
                 .indexes   - List all indexes\n\
                 .format    - Set output format (table, csv, json)\n\
                 .exit      - Exit the REPL"
                    .into(),
            )),
        }
    }

    pub fn evaluate_expr(
        &self,
        expr: &Expr,
        row: &[Value],
        table_map: &HashMap<String, usize>,
    ) -> Result<Value> {
        match expr {
            Expr::Literal(v) => Ok(v.clone()),
            Expr::Column { table, name } => {
                if let Some(tbl) = table {
                    // Qualified column reference (table.column)
                    if let Some(&offset) = table_map.get(tbl) {
                        // In a joined context, use the table_map offset
                        if let Some(table_meta) = self.catalog.tables.get(tbl) {
                            if let Some(idx) =
                                table_meta.columns.iter().position(|c| &c.name == name)
                            {
                                let pos = offset + idx;
                                if pos < row.len() {
                                    return Ok(row[pos].clone());
                                }
                            }
                        }
                    } else {
                        // Single table context
                        if let Some(table_meta) = self.catalog.tables.get(tbl) {
                            if let Some(idx) =
                                table_meta.columns.iter().position(|c| &c.name == name)
                            {
                                if idx < row.len() {
                                    return Ok(row[idx].clone());
                                }
                            }
                        }
                    }
                } else {
                    // Unqualified column - search all tables in the table_map (or all tables)
                    if !table_map.is_empty() {
                        for (tbl, &offset) in table_map {
                            if let Some(table_meta) = self.catalog.tables.get(tbl) {
                                if let Some(idx) =
                                    table_meta.columns.iter().position(|c| &c.name == name)
                                {
                                    let pos = offset + idx;
                                    if pos < row.len() {
                                        return Ok(row[pos].clone());
                                    }
                                }
                            }
                        }
                    } else {
                        for table_meta in self.catalog.tables.values() {
                            if let Some(idx) =
                                table_meta.columns.iter().position(|c| &c.name == name)
                            {
                                if idx < row.len() {
                                    return Ok(row[idx].clone());
                                }
                            }
                        }
                    }
                }
                Ok(Value::Null)
            }
            Expr::BinaryOp { left, op, right } => {
                let lv = self.evaluate_expr(left, row, table_map)?;
                let rv = self.evaluate_expr(right, row, table_map)?;
                match op {
                    BinaryOp::Eq => Ok(tvl_eq(&lv, &rv)),
                    BinaryOp::Neq => Ok(tvl_neq(&lv, &rv)),
                    BinaryOp::Lt => Ok(tvl_lt(&lv, &rv)),
                    BinaryOp::Le => Ok(tvl_le(&lv, &rv)),
                    BinaryOp::Gt => Ok(tvl_gt(&lv, &rv)),
                    BinaryOp::Ge => Ok(tvl_ge(&lv, &rv)),
                    BinaryOp::Add => tvl_add(&lv, &rv),
                    BinaryOp::Sub => tvl_sub(&lv, &rv),
                    BinaryOp::Mul => tvl_mul(&lv, &rv),
                    BinaryOp::Div => tvl_div(&lv, &rv),
                    BinaryOp::Concat => tvl_concat(&lv, &rv),
                    BinaryOp::And => Ok(tvl_and(&lv, &rv)),
                    BinaryOp::Or => Ok(tvl_or(&lv, &rv)),
                }
            }
            Expr::UnaryOp { op, expr } => {
                let v = self.evaluate_expr(expr, row, table_map)?;
                match op {
                    UnaryOp::Not => Ok(tvl_not(&v)),
                    UnaryOp::Neg => match v {
                        Value::Int(n) => Ok(Value::Int(-n)),
                        Value::Real(n) => Ok(Value::Real(-n)),
                        Value::Null => Ok(Value::Null),
                        _ => Err(MiniDbError::Type {
                            message: format!("cannot negate {}", v.type_name()),
                        }),
                    },
                }
            }
            Expr::Function { name, args } => self.evaluate_function(name, args, row, table_map),
            Expr::IsNull(expr) => {
                let v = self.evaluate_expr(expr, row, table_map)?;
                Ok(Value::Bool(v.is_null()))
            }
            Expr::IsNotNull(expr) => {
                let v = self.evaluate_expr(expr, row, table_map)?;
                Ok(Value::Bool(!v.is_null()))
            }
        }
    }

    fn evaluate_function(
        &self,
        name: &str,
        args: &[Expr],
        row: &[Value],
        table_map: &HashMap<String, usize>,
    ) -> Result<Value> {
        let evaluated: Vec<Value> = args
            .iter()
            .map(|a| self.evaluate_expr(a, row, table_map))
            .collect::<Result<Vec<_>>>()?;

        match name.to_uppercase().as_str() {
            "LENGTH" => {
                if let Some(Value::Text(s)) = evaluated.first() {
                    Ok(Value::Int(s.len() as i64))
                } else if let Some(Value::Null) = evaluated.first() {
                    Ok(Value::Null)
                } else {
                    Err(MiniDbError::Type {
                        message: "LENGTH requires a text argument".into(),
                    })
                }
            }
            "LOWER" => {
                if let Some(Value::Text(s)) = evaluated.first() {
                    Ok(Value::Text(s.to_lowercase()))
                } else {
                    Ok(Value::Null)
                }
            }
            "UPPER" => {
                if let Some(Value::Text(s)) = evaluated.first() {
                    Ok(Value::Text(s.to_uppercase()))
                } else {
                    Ok(Value::Null)
                }
            }
            "SUBSTR" | "SUBSTRING" => {
                if evaluated.len() < 2 {
                    return Err(MiniDbError::Runtime {
                        message: "SUBSTR requires at least 2 arguments".into(),
                    });
                }
                match (&evaluated[0], &evaluated[1]) {
                    (Value::Text(s), Value::Int(start)) => {
                        let start = (*start).max(0) as usize;
                        let len = if evaluated.len() > 2 {
                            match &evaluated[2] {
                                Value::Int(n) => (*n).max(0) as usize,
                                _ => s.len(),
                            }
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
                    (Value::Null, _) | (_, Value::Int(_)) if evaluated[0].is_null() => {
                        Ok(Value::Null)
                    }
                    _ => Err(MiniDbError::Type {
                        message: "SUBSTR requires (TEXT, INT [, INT])".into(),
                    }),
                }
            }
            "ABS" => match evaluated.first() {
                Some(Value::Int(n)) => Ok(Value::Int(n.abs())),
                Some(Value::Real(n)) => Ok(Value::Real(n.abs())),
                Some(Value::Null) => Ok(Value::Null),
                _ => Err(MiniDbError::Type {
                    message: "ABS requires a numeric argument".into(),
                }),
            },
            "COALESCE" => {
                for v in &evaluated {
                    if !v.is_null() {
                        return Ok(v.clone());
                    }
                }
                Ok(Value::Null)
            }
            _ => Err(MiniDbError::Runtime {
                message: format!("unknown function '{}'", name),
            }),
        }
    }
}

#[derive(Debug)]
pub enum ExecResult {
    Rows {
        columns: Vec<String>,
        rows: Vec<Row>,
    },
    RowsAffected(u64),
    Message(String),
    SetFormat(String),
    Exit,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_engine() -> Engine {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut storage = Storage::open(&path).unwrap();
        let catalog = Catalog::load(&mut storage.pm).unwrap();
        Engine::new(storage, catalog)
    }

    #[test]
    fn test_evaluate_literal() {
        let engine = setup_engine();
        let v = engine
            .evaluate_expr(&Expr::Literal(Value::Int(42)), &[], &HashMap::new())
            .unwrap();
        assert_eq!(v, Value::Int(42));
    }

    #[test]
    fn test_evaluate_binary() {
        let engine = setup_engine();
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Literal(Value::Int(1))),
            op: BinaryOp::Add,
            right: Box::new(Expr::Literal(Value::Int(2))),
        };
        let v = engine.evaluate_expr(&expr, &[], &HashMap::new()).unwrap();
        assert_eq!(v, Value::Int(3));
    }

    #[test]
    fn test_evaluate_function() {
        let engine = setup_engine();
        let expr = Expr::Function {
            name: "LENGTH".into(),
            args: vec![Expr::Literal(Value::Text("hello".into()))],
        };
        let v = engine.evaluate_expr(&expr, &[], &HashMap::new()).unwrap();
        assert_eq!(v, Value::Int(5));
    }

    #[test]
    fn test_evaluate_coalesce() {
        let engine = setup_engine();
        let expr = Expr::Function {
            name: "COALESCE".into(),
            args: vec![
                Expr::Literal(Value::Null),
                Expr::Literal(Value::Text("default".into())),
            ],
        };
        let v = engine.evaluate_expr(&expr, &[], &HashMap::new()).unwrap();
        assert_eq!(v, Value::Text("default".into()));
    }
}

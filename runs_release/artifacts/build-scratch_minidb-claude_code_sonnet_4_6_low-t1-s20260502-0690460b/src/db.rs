/// Database engine: ties together storage, catalog, WAL, txn, planner, executor
use crate::catalog::{Catalog, IndexMeta, TableSchema};
use crate::error::{MiniDbError, Result};
use crate::exec::eval::Evaluator;
use crate::exec::operators::*;
use crate::parser::ast::*;
use crate::parser::Parser;
use crate::plan::{IndexPredicate, LogicalPlan, OutputColumn, Planner, ProjectItem, Schema};
use crate::storage::btree::BTree;
use crate::storage::heap::{deserialize_row, serialize_row, Heap};
use crate::storage::wal::{replay_committed, Wal, WalRecord};
use crate::txn::{TxnManager, UndoEntry};
use crate::types::{ColumnDef, DataType, Row, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Result of executing a statement
pub struct ExecResult {
    pub schema: Schema,
    pub rows: Vec<Row>,
    pub rows_affected: usize,
    pub kind: ResultKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResultKind {
    Select,
    Insert,
    Update,
    Delete,
    DDL,
    Txn,
    Empty,
}

pub struct Database {
    path: PathBuf,
    catalog: Catalog,
    heaps: HashMap<String, Heap>,
    btrees: HashMap<String, BTree>, // index_name -> btree
    wal: Wal,
    txn: TxnManager,
    auto_commit: bool,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let wal_path = path.with_extension("wal");
        let mut wal = Wal::open(&wal_path)?;

        // Replay WAL
        let records = Wal::read_all(&wal_path)?;
        let committed = replay_committed(&records);

        // Load catalog from file or build empty
        let catalog_path = path.with_extension("catalog");
        let mut catalog = if catalog_path.exists() {
            let data = std::fs::read(&catalog_path)?;
            Catalog::deserialize(&data)?
        } else {
            Catalog::new()
        };

        // Load heaps from files
        let mut heaps: HashMap<String, Heap> = HashMap::new();
        let mut btrees: HashMap<String, BTree> = HashMap::new();

        for tbl_name in catalog.tables.keys().cloned().collect::<Vec<_>>() {
            let heap_path = heap_path(path, &tbl_name);
            let heap = if heap_path.exists() {
                let data = std::fs::read(&heap_path)?;
                Heap::from_bytes(&data)?
            } else {
                Heap::new()
            };
            heaps.insert(tbl_name, heap);
        }

        for idx_name in catalog.indexes.keys().cloned().collect::<Vec<_>>() {
            let bt_path = btree_path(path, &idx_name);
            let bt = if bt_path.exists() {
                let data = std::fs::read(&bt_path)?;
                BTree::from_bytes(&data)?
            } else {
                BTree::new()
            };
            btrees.insert(idx_name, bt);
        }

        let mut db = Database {
            path: path.to_path_buf(),
            catalog,
            heaps,
            btrees,
            wal,
            txn: TxnManager::new(),
            auto_commit: true,
        };

        // Replay committed WAL records
        let mut max_txn_id = 0u64;
        for record in &committed {
            max_txn_id = max_txn_id.max(record.txn_id());
            db.apply_wal_record(record)?;
        }
        db.txn.set_min_txn_id(max_txn_id);

        // Persist state after replay
        if !records.is_empty() {
            db.persist_all()?;
            db.wal.truncate()?;
        }

        Ok(db)
    }

    fn apply_wal_record(&mut self, record: &WalRecord) -> Result<()> {
        match record {
            WalRecord::InsertRow {
                table,
                row_id,
                data,
                ..
            } => {
                self.heaps
                    .entry(table.clone())
                    .or_insert_with(Heap::new)
                    .insert_row_with_id(*row_id, data.clone())?;
                if let Ok(row) = deserialize_row(data) {
                    self.update_indexes_insert(table, *row_id, &row);
                }
            }
            WalRecord::DeleteRow { table, row_id, .. } => {
                let old_data = self
                    .heaps
                    .get(table)
                    .and_then(|h| h.get_row_data(*row_id))
                    .map(|d| d.to_vec());
                if let Some(data) = old_data {
                    if let Ok(row) = deserialize_row(&data) {
                        self.update_indexes_delete(table, *row_id, &row);
                    }
                }
                if let Some(heap) = self.heaps.get_mut(table) {
                    let _ = heap.delete_row(*row_id);
                }
            }
            WalRecord::UpdateRow {
                table,
                row_id,
                new_data,
                old_data,
                ..
            } => {
                if let Ok(old_row) = deserialize_row(old_data) {
                    self.update_indexes_delete(table, *row_id, &old_row);
                }
                if let Ok(new_row) = deserialize_row(new_data) {
                    if let Some(heap) = self.heaps.get_mut(table) {
                        let _ = heap.update_row(*row_id, &new_row);
                    }
                    self.update_indexes_insert(table, *row_id, &new_row);
                }
            }
            WalRecord::Catalog { data, .. } => {
                let cat = Catalog::deserialize(data)?;
                // Merge: add new tables/indexes
                for (name, schema) in &cat.tables {
                    if !self.catalog.tables.contains_key(name) {
                        self.catalog.tables.insert(name.clone(), schema.clone());
                        self.heaps.entry(name.clone()).or_insert_with(Heap::new);
                    }
                }
                for (name, meta) in &cat.indexes {
                    if !self.catalog.indexes.contains_key(name) {
                        self.catalog.indexes.insert(name.clone(), meta.clone());
                        self.btrees.entry(name.clone()).or_insert_with(BTree::new);
                    }
                }
                // Handle drops
                let existing_tables: Vec<_> = self.catalog.tables.keys().cloned().collect();
                for tbl in existing_tables {
                    if !cat.tables.contains_key(&tbl) {
                        self.catalog.tables.remove(&tbl);
                        self.heaps.remove(&tbl);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn update_indexes_insert(&mut self, table: &str, row_id: u64, row: &Row) {
        let indexes: Vec<_> = self
            .catalog
            .indexes_for_table(table)
            .iter()
            .map(|m| (m.name.clone(), m.column.clone()))
            .collect();
        for (idx_name, col_name) in indexes {
            let col_idx = self
                .catalog
                .tables
                .get(table)
                .and_then(|t| t.col_index(&col_name));
            if let Some(ci) = col_idx {
                let val = row.get(ci).cloned().unwrap_or(Value::Null);
                if let Some(bt) = self.btrees.get_mut(&idx_name) {
                    bt.insert(val, row_id);
                }
            }
        }
    }

    fn update_indexes_delete(&mut self, table: &str, row_id: u64, row: &Row) {
        let indexes: Vec<_> = self
            .catalog
            .indexes_for_table(table)
            .iter()
            .map(|m| (m.name.clone(), m.column.clone()))
            .collect();
        for (idx_name, col_name) in indexes {
            let col_idx = self
                .catalog
                .tables
                .get(table)
                .and_then(|t| t.col_index(&col_name));
            if let Some(ci) = col_idx {
                let val = row.get(ci).cloned().unwrap_or(Value::Null);
                if let Some(bt) = self.btrees.get_mut(&idx_name) {
                    bt.remove(&val, row_id);
                }
            }
        }
    }

    pub fn execute_sql(&mut self, sql: &str) -> Result<Vec<ExecResult>> {
        let stmts = Parser::parse(sql)?;
        let mut results = Vec::new();
        for stmt in stmts {
            let res = self.execute_statement(stmt)?;
            results.push(res);
        }
        Ok(results)
    }

    pub fn execute_statement(&mut self, stmt: Statement) -> Result<ExecResult> {
        match &stmt {
            Statement::Begin => {
                self.txn.begin(&mut self.wal)?;
                self.auto_commit = false;
                return Ok(ExecResult {
                    schema: Schema::new(vec![]),
                    rows: vec![],
                    rows_affected: 0,
                    kind: ResultKind::Txn,
                });
            }
            Statement::Commit => {
                if self.txn.is_active() {
                    self.txn.commit(&mut self.wal)?;
                    self.persist_all()?;
                }
                self.auto_commit = true;
                return Ok(ExecResult {
                    schema: Schema::new(vec![]),
                    rows: vec![],
                    rows_affected: 0,
                    kind: ResultKind::Txn,
                });
            }
            Statement::Rollback => {
                if self.txn.is_active() {
                    let undo = self.txn.rollback(&mut self.wal)?;
                    self.apply_undo(undo)?;
                }
                self.auto_commit = true;
                return Ok(ExecResult {
                    schema: Schema::new(vec![]),
                    rows: vec![],
                    rows_affected: 0,
                    kind: ResultKind::Txn,
                });
            }
            Statement::MetaSchema
            | Statement::MetaTables
            | Statement::MetaIndexes
            | Statement::MetaExit
            | Statement::MetaFormat(_) => {
                return self.execute_meta(&stmt);
            }
            _ => {}
        }

        // Auto-begin if needed
        let was_auto = if !self.txn.is_active() {
            self.txn.begin(&mut self.wal)?;
            true
        } else {
            false
        };

        let result = self.execute_dml_ddl(stmt);

        match result {
            Ok(res) => {
                if was_auto {
                    self.txn.commit(&mut self.wal)?;
                    self.persist_all()?;
                }
                Ok(res)
            }
            Err(e) => {
                if was_auto && self.txn.is_active() {
                    let undo = self.txn.rollback(&mut self.wal).unwrap_or_default();
                    let _ = self.apply_undo(undo);
                }
                Err(e)
            }
        }
    }

    fn execute_dml_ddl(&mut self, stmt: Statement) -> Result<ExecResult> {
        match stmt {
            Statement::Select(sel) => self.execute_select(&sel),
            Statement::Insert(ins) => self.execute_insert(&ins),
            Statement::Update(upd) => self.execute_update(&upd),
            Statement::Delete(del) => self.execute_delete(&del),
            Statement::CreateTable(ct) => self.execute_create_table(&ct),
            Statement::DropTable(dt) => self.execute_drop_table(&dt),
            Statement::CreateIndex(ci) => self.execute_create_index(&ci),
            Statement::DropIndex(di) => self.execute_drop_index(&di),
            _ => Err(MiniDbError::Runtime("unexpected statement".into())),
        }
    }

    fn execute_select(&self, sel: &SelectStmt) -> Result<ExecResult> {
        // Plan
        let planner = Planner::new(&self.catalog);
        let plan = planner.plan_statement(&Statement::Select(sel.clone()))?;

        // Build operator tree
        let mut op = self.build_operator(plan)?;
        op.open()?;

        let schema = op.schema().clone();
        let mut rows = Vec::new();
        while let Some(row) = op.next()? {
            rows.push(row);
        }
        op.close()?;

        Ok(ExecResult {
            schema,
            rows,
            rows_affected: 0,
            kind: ResultKind::Select,
        })
    }

    fn build_operator(&self, plan: LogicalPlan) -> Result<Box<dyn PhysicalOperator>> {
        match plan {
            LogicalPlan::Scan { table } => {
                let schema_def = self.catalog.get_table(&table)?.columns.clone();
                let heap = self.heaps.get(&table).ok_or_else(|| {
                    MiniDbError::Storage(format!("heap for '{}' not found", table))
                })?;
                Ok(Box::new(SeqScan::new(&table, heap, &schema_def)))
            }
            LogicalPlan::IndexScan {
                table,
                index,
                column,
                predicate,
            } => {
                let schema_def = self.catalog.get_table(&table)?.columns.clone();
                let heap = self.heaps.get(&table).ok_or_else(|| {
                    MiniDbError::Storage(format!("heap for '{}' not found", table))
                })?;
                let btree = self.btrees.get(&index).ok_or_else(|| {
                    MiniDbError::Storage(format!("btree for '{}' not found", index))
                })?;
                Ok(Box::new(IndexScan::new(
                    &table,
                    heap,
                    btree,
                    &predicate,
                    &schema_def,
                )))
            }
            LogicalPlan::Filter { input, predicate } => {
                let child = self.build_operator(*input)?;
                Ok(Box::new(Filter::new(child, predicate)))
            }
            LogicalPlan::Project { input, columns } => {
                let child = self.build_operator(*input)?;
                Ok(Box::new(Project::new(child, columns)))
            }
            LogicalPlan::Join {
                left,
                right,
                condition,
            } => {
                let left_op = self.build_operator(*left)?;
                let right_op = self.build_operator(*right)?;
                let right_schema = right_op.schema().clone();
                // Materialize right side
                let mut right_op = right_op;
                right_op.open()?;
                let mut right_rows = Vec::new();
                while let Some(r) = right_op.next()? {
                    right_rows.push(r);
                }
                right_op.close()?;
                Ok(Box::new(NestedLoopJoinBuilder::new(
                    left_op,
                    right_rows,
                    right_schema,
                    condition,
                )))
            }
            LogicalPlan::Sort { input, keys } => {
                let child = self.build_operator(*input)?;
                Ok(Box::new(Sort::new(child, keys)))
            }
            LogicalPlan::Limit {
                input,
                limit,
                offset,
            } => {
                let child = self.build_operator(*input)?;
                Ok(Box::new(Limit::new(child, limit, offset)))
            }
            LogicalPlan::Values { rows } => {
                let empty_schema = Schema::new(vec![]);
                let eval_rows: Vec<Row> = rows
                    .iter()
                    .map(|exprs| {
                        exprs
                            .iter()
                            .map(|e| Evaluator::eval(e, &[], &empty_schema))
                            .collect::<Result<Row>>()
                    })
                    .collect::<Result<Vec<Row>>>()?;
                let ncols = eval_rows.first().map(|r| r.len()).unwrap_or(0);
                let schema = Schema::new(
                    (0..ncols)
                        .map(|i| OutputColumn {
                            name: format!("col{}", i),
                            table: None,
                            dtype: None,
                        })
                        .collect(),
                );
                Ok(Box::new(ValuesScan::new(eval_rows, schema)))
            }
            _ => Err(MiniDbError::Runtime(
                "cannot build operator for DML plan".into(),
            )),
        }
    }

    fn execute_insert(&mut self, ins: &InsertStmt) -> Result<ExecResult> {
        let schema = self.catalog.get_table(&ins.table)?.clone();
        let txn_id = self.txn.active_id().unwrap_or(0);
        let empty_eval_schema = Schema::new(vec![]);
        let mut total = 0usize;

        for value_row in &ins.values {
            let values: Vec<Value> = value_row
                .iter()
                .map(|e| Evaluator::eval(e, &[], &empty_eval_schema))
                .collect::<Result<_>>()?;

            // Build full row
            let named: Vec<(String, Value)> = if let Some(cols) = &ins.columns {
                cols.iter()
                    .zip(values.iter())
                    .map(|(c, v)| (c.clone(), v.clone()))
                    .collect()
            } else {
                schema
                    .columns
                    .iter()
                    .zip(values.iter())
                    .map(|(c, v)| (c.name.clone(), v.clone()))
                    .collect()
            };

            let row = schema.build_row(&named)?;

            // Check UNIQUE constraints
            self.check_unique_constraints(&ins.table, &schema, &row, None)?;
            // Check CHECK constraints (simplified - skip for now as we'd need expression storage)

            let data = serialize_row(&row)?;
            let heap = self
                .heaps
                .entry(ins.table.clone())
                .or_insert_with(Heap::new);
            let row_id = heap.insert_row(&row)?;

            self.wal.append(&WalRecord::InsertRow {
                txn_id,
                table: ins.table.clone(),
                row_id,
                data: data.clone(),
            })?;

            self.txn.log_insert(&ins.table, row_id);

            // Update indexes
            self.update_indexes_insert(&ins.table, row_id, &row);
            total += 1;
        }

        Ok(ExecResult {
            schema: Schema::new(vec![]),
            rows: vec![],
            rows_affected: total,
            kind: ResultKind::Insert,
        })
    }

    fn check_unique_constraints(
        &self,
        table: &str,
        schema: &TableSchema,
        row: &Row,
        exclude_row_id: Option<u64>,
    ) -> Result<()> {
        let heap = self.heaps.get(table);

        for (i, col) in schema.columns.iter().enumerate() {
            if !col.unique && !col.primary_key {
                continue;
            }
            let val = &row[i];
            if val.is_null() {
                continue;
            }

            // Check via index if available
            let idx = self
                .catalog
                .indexes_for_table(table)
                .into_iter()
                .find(|m| m.column.eq_ignore_ascii_case(&col.name));

            if let Some(idx) = idx {
                if let Some(bt) = self.btrees.get(&idx.name) {
                    let hits = bt.lookup_eq(val);
                    for &rid in &hits {
                        if exclude_row_id == Some(rid) {
                            continue;
                        }
                        return Err(MiniDbError::Constraint(format!(
                            "UNIQUE constraint violated on column '{}'",
                            col.name
                        )));
                    }
                }
            } else {
                // Scan heap
                if let Some(heap) = heap {
                    for (rid, existing) in heap.scan() {
                        if exclude_row_id == Some(rid) {
                            continue;
                        }
                        if existing
                            .get(i)
                            .map(|v| v.sql_eq(val) == Some(true))
                            .unwrap_or(false)
                        {
                            return Err(MiniDbError::Constraint(format!(
                                "UNIQUE constraint violated on column '{}'",
                                col.name
                            )));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn execute_update(&mut self, upd: &UpdateStmt) -> Result<ExecResult> {
        let schema = self.catalog.get_table(&upd.table)?.clone();
        let txn_id = self.txn.active_id().unwrap_or(0);
        let col_schema = Schema::new(
            schema
                .columns
                .iter()
                .map(|c| OutputColumn {
                    name: c.name.clone(),
                    table: Some(upd.table.clone()),
                    dtype: Some(c.dtype.clone()),
                })
                .collect(),
        );

        let heap = self
            .heaps
            .get(&upd.table)
            .ok_or_else(|| MiniDbError::TableNotFound(upd.table.clone()))?;

        // Find matching rows
        let matching: Vec<(u64, Row)> = heap
            .scan()
            .filter(|(_, row)| {
                if let Some(pred) = &upd.where_clause {
                    Evaluator::eval_bool(pred, row, &col_schema).unwrap_or(false)
                } else {
                    true
                }
            })
            .collect();

        let mut affected = 0usize;
        for (row_id, mut row) in matching {
            let old_data = serialize_row(&row)?;

            // Apply assignments
            for (col_name, expr) in &upd.assignments {
                let ci = schema
                    .col_index(col_name)
                    .ok_or_else(|| MiniDbError::ColumnNotFound(col_name.clone()))?;
                row[ci] = Evaluator::eval(expr, &row, &col_schema)?;
                // Coerce type
                row[ci] = row[ci].coerce_to(&schema.columns[ci].dtype)?;
            }

            // Check NOT NULL
            for (i, col) in schema.columns.iter().enumerate() {
                if col.not_null && row[i].is_null() {
                    return Err(MiniDbError::Constraint(format!(
                        "column '{}' cannot be NULL",
                        col.name
                    )));
                }
            }

            // Check UNIQUE
            self.check_unique_constraints(&upd.table, &schema, &row, Some(row_id))?;

            let new_data = serialize_row(&row)?;

            // Remove old index entries
            let old_row = deserialize_row(&old_data)?;
            self.update_indexes_delete(&upd.table, row_id, &old_row);

            let heap = self.heaps.get_mut(&upd.table).unwrap();
            heap.update_row(row_id, &row)?;

            self.wal.append(&WalRecord::UpdateRow {
                txn_id,
                table: upd.table.clone(),
                row_id,
                old_data: old_data.clone(),
                new_data: new_data.clone(),
            })?;
            self.txn.log_update(&upd.table, row_id, old_data);

            // Add new index entries
            self.update_indexes_insert(&upd.table, row_id, &row);
            affected += 1;
        }

        Ok(ExecResult {
            schema: Schema::new(vec![]),
            rows: vec![],
            rows_affected: affected,
            kind: ResultKind::Update,
        })
    }

    fn execute_delete(&mut self, del: &DeleteStmt) -> Result<ExecResult> {
        let schema = self.catalog.get_table(&del.table)?.clone();
        let txn_id = self.txn.active_id().unwrap_or(0);
        let col_schema = Schema::new(
            schema
                .columns
                .iter()
                .map(|c| OutputColumn {
                    name: c.name.clone(),
                    table: Some(del.table.clone()),
                    dtype: Some(c.dtype.clone()),
                })
                .collect(),
        );

        let heap = self
            .heaps
            .get(&del.table)
            .ok_or_else(|| MiniDbError::TableNotFound(del.table.clone()))?;

        let matching: Vec<(u64, Vec<u8>)> = heap
            .scan_raw()
            .filter(|slot| {
                if let Some(pred) = &del.where_clause {
                    if let Ok(row) = deserialize_row(&slot.data) {
                        Evaluator::eval_bool(pred, &row, &col_schema).unwrap_or(false)
                    } else {
                        false
                    }
                } else {
                    true
                }
            })
            .map(|slot| (slot.row_id, slot.data.clone()))
            .collect();

        let mut affected = 0usize;
        for (row_id, data) in matching {
            // Update indexes
            if let Ok(row) = deserialize_row(&data) {
                self.update_indexes_delete(&del.table, row_id, &row);
            }

            let heap = self.heaps.get_mut(&del.table).unwrap();
            heap.delete_row(row_id)?;

            self.wal.append(&WalRecord::DeleteRow {
                txn_id,
                table: del.table.clone(),
                row_id,
            })?;
            self.txn.log_delete(&del.table, row_id, data);
            affected += 1;
        }

        Ok(ExecResult {
            schema: Schema::new(vec![]),
            rows: vec![],
            rows_affected: affected,
            kind: ResultKind::Delete,
        })
    }

    fn execute_create_table(&mut self, ct: &CreateTableStmt) -> Result<ExecResult> {
        if ct.if_not_exists && self.catalog.tables.contains_key(&ct.name) {
            return Ok(ExecResult {
                schema: Schema::new(vec![]),
                rows: vec![],
                rows_affected: 0,
                kind: ResultKind::DDL,
            });
        }
        let cols: Vec<ColumnDef> =
            ct.columns
                .iter()
                .map(|c| {
                    let default = c.default.as_ref().map(|e| {
                        Evaluator::eval(e, &[], &Schema::new(vec![])).unwrap_or(Value::Null)
                    });
                    ColumnDef {
                        name: c.name.clone(),
                        dtype: c.dtype.clone(),
                        not_null: c.not_null,
                        unique: c.unique,
                        primary_key: c.primary_key,
                        default,
                    }
                })
                .collect();
        let schema = TableSchema::new(ct.name.clone(), cols);
        self.catalog.create_table(schema)?;
        self.heaps.insert(ct.name.clone(), Heap::new());

        let txn_id = self.txn.active_id().unwrap_or(0);
        let cat_bytes = self.catalog.serialize()?;
        self.wal.append(&WalRecord::Catalog {
            txn_id,
            data: cat_bytes,
        })?;

        Ok(ExecResult {
            schema: Schema::new(vec![]),
            rows: vec![],
            rows_affected: 0,
            kind: ResultKind::DDL,
        })
    }

    fn execute_drop_table(&mut self, dt: &DropTableStmt) -> Result<ExecResult> {
        if dt.if_exists && !self.catalog.tables.contains_key(&dt.name) {
            return Ok(ExecResult {
                schema: Schema::new(vec![]),
                rows: vec![],
                rows_affected: 0,
                kind: ResultKind::DDL,
            });
        }
        self.catalog.drop_table(&dt.name)?;
        self.heaps.remove(&dt.name);

        let txn_id = self.txn.active_id().unwrap_or(0);
        let cat_bytes = self.catalog.serialize()?;
        self.wal.append(&WalRecord::Catalog {
            txn_id,
            data: cat_bytes,
        })?;

        Ok(ExecResult {
            schema: Schema::new(vec![]),
            rows: vec![],
            rows_affected: 0,
            kind: ResultKind::DDL,
        })
    }

    fn execute_create_index(&mut self, ci: &CreateIndexStmt) -> Result<ExecResult> {
        if ci.if_not_exists && self.catalog.indexes.contains_key(&ci.name) {
            return Ok(ExecResult {
                schema: Schema::new(vec![]),
                rows: vec![],
                rows_affected: 0,
                kind: ResultKind::DDL,
            });
        }
        // Validate table and column exist
        let tbl = self.catalog.get_table(&ci.table)?;
        let col_idx = tbl
            .col_index(&ci.column)
            .ok_or_else(|| MiniDbError::ColumnNotFound(ci.column.clone()))?;

        let meta = IndexMeta {
            name: ci.name.clone(),
            table: ci.table.clone(),
            column: ci.column.clone(),
            unique: ci.unique,
            root_page: 0,
        };
        self.catalog.create_index(meta)?;

        // Build index from existing data
        let mut bt = BTree::new();
        if let Some(heap) = self.heaps.get(&ci.table) {
            for (row_id, row) in heap.scan() {
                let val = row.get(col_idx).cloned().unwrap_or(Value::Null);
                bt.insert(val, row_id);
            }
        }
        self.btrees.insert(ci.name.clone(), bt);

        let txn_id = self.txn.active_id().unwrap_or(0);
        let cat_bytes = self.catalog.serialize()?;
        self.wal.append(&WalRecord::Catalog {
            txn_id,
            data: cat_bytes,
        })?;

        Ok(ExecResult {
            schema: Schema::new(vec![]),
            rows: vec![],
            rows_affected: 0,
            kind: ResultKind::DDL,
        })
    }

    fn execute_drop_index(&mut self, di: &DropIndexStmt) -> Result<ExecResult> {
        if di.if_exists && !self.catalog.indexes.contains_key(&di.name) {
            return Ok(ExecResult {
                schema: Schema::new(vec![]),
                rows: vec![],
                rows_affected: 0,
                kind: ResultKind::DDL,
            });
        }
        self.catalog.drop_index(&di.name)?;
        self.btrees.remove(&di.name);

        let txn_id = self.txn.active_id().unwrap_or(0);
        let cat_bytes = self.catalog.serialize()?;
        self.wal.append(&WalRecord::Catalog {
            txn_id,
            data: cat_bytes,
        })?;

        Ok(ExecResult {
            schema: Schema::new(vec![]),
            rows: vec![],
            rows_affected: 0,
            kind: ResultKind::DDL,
        })
    }

    fn execute_meta(&self, stmt: &Statement) -> Result<ExecResult> {
        match stmt {
            Statement::MetaSchema => {
                let schema = Schema::new(vec![OutputColumn {
                    name: "sql".into(),
                    table: None,
                    dtype: None,
                }]);
                let mut rows = Vec::new();
                for (name, tbl) in &self.catalog.tables {
                    let cols: Vec<String> = tbl
                        .columns
                        .iter()
                        .map(|c| {
                            let mut s = format!("  {} {}", c.name, c.dtype);
                            if c.primary_key {
                                s += " PRIMARY KEY";
                            }
                            if c.not_null && !c.primary_key {
                                s += " NOT NULL";
                            }
                            if c.unique && !c.primary_key {
                                s += " UNIQUE";
                            }
                            s
                        })
                        .collect();
                    let sql = format!("CREATE TABLE {} (\n{}\n);", name, cols.join(",\n"));
                    rows.push(vec![Value::Text(sql)]);
                }
                for (idx_name, meta) in &self.catalog.indexes {
                    let sql = format!(
                        "CREATE INDEX {} ON {} ({});",
                        idx_name, meta.table, meta.column
                    );
                    rows.push(vec![Value::Text(sql)]);
                }
                Ok(ExecResult {
                    schema,
                    rows,
                    rows_affected: 0,
                    kind: ResultKind::Select,
                })
            }
            Statement::MetaTables => {
                let schema = Schema::new(vec![OutputColumn {
                    name: "name".into(),
                    table: None,
                    dtype: None,
                }]);
                let rows: Vec<Row> = self
                    .catalog
                    .tables
                    .keys()
                    .map(|n| vec![Value::Text(n.clone())])
                    .collect();
                Ok(ExecResult {
                    schema,
                    rows,
                    rows_affected: 0,
                    kind: ResultKind::Select,
                })
            }
            Statement::MetaIndexes => {
                let schema = Schema::new(vec![
                    OutputColumn {
                        name: "name".into(),
                        table: None,
                        dtype: None,
                    },
                    OutputColumn {
                        name: "table".into(),
                        table: None,
                        dtype: None,
                    },
                    OutputColumn {
                        name: "column".into(),
                        table: None,
                        dtype: None,
                    },
                ]);
                let rows: Vec<Row> = self
                    .catalog
                    .indexes
                    .values()
                    .map(|m| {
                        vec![
                            Value::Text(m.name.clone()),
                            Value::Text(m.table.clone()),
                            Value::Text(m.column.clone()),
                        ]
                    })
                    .collect();
                Ok(ExecResult {
                    schema,
                    rows,
                    rows_affected: 0,
                    kind: ResultKind::Select,
                })
            }
            _ => Ok(ExecResult {
                schema: Schema::new(vec![]),
                rows: vec![],
                rows_affected: 0,
                kind: ResultKind::Empty,
            }),
        }
    }

    fn apply_undo(&mut self, entries: Vec<UndoEntry>) -> Result<()> {
        for entry in entries.into_iter().rev() {
            match entry {
                UndoEntry::InsertRow { table, row_id } => {
                    // Get row data for index removal
                    if let Some(heap) = self.heaps.get(&table) {
                        if let Some(data) = heap.get_row_data(row_id) {
                            let data = data.to_vec();
                            if let Ok(row) = deserialize_row(&data) {
                                self.update_indexes_delete(&table, row_id, &row);
                            }
                        }
                    }
                    if let Some(heap) = self.heaps.get_mut(&table) {
                        heap.undo_insert(row_id);
                    }
                }
                UndoEntry::DeleteRow {
                    table,
                    row_id,
                    data,
                } => {
                    if let Ok(row) = deserialize_row(&data) {
                        self.update_indexes_insert(&table, row_id, &row);
                    }
                    if let Some(heap) = self.heaps.get_mut(&table) {
                        heap.undo_delete(row_id, data);
                    }
                }
                UndoEntry::UpdateRow {
                    table,
                    row_id,
                    old_data,
                } => {
                    // Remove new index entries
                    if let Some(heap) = self.heaps.get(&table) {
                        if let Some(data) = heap.get_row_data(row_id) {
                            let data = data.to_vec();
                            if let Ok(row) = deserialize_row(&data) {
                                self.update_indexes_delete(&table, row_id, &row);
                            }
                        }
                    }
                    if let Ok(old_row) = deserialize_row(&old_data) {
                        self.update_indexes_insert(&table, row_id, &old_row);
                    }
                    if let Some(heap) = self.heaps.get_mut(&table) {
                        heap.undo_update(row_id, old_data);
                    }
                }
            }
        }
        Ok(())
    }

    fn persist_all(&self) -> Result<()> {
        // Persist catalog
        let cat_bytes = self.catalog.serialize()?;
        let cat_path = self.path.with_extension("catalog");
        std::fs::write(&cat_path, &cat_bytes)?;

        // Persist each heap
        for (name, heap) in &self.heaps {
            let hp = heap_path(&self.path, name);
            std::fs::write(&hp, heap.to_bytes())?;
        }

        // Persist each btree
        for (name, bt) in &self.btrees {
            let bp = btree_path(&self.path, name);
            std::fs::write(&bp, bt.to_bytes()?)?;
        }
        Ok(())
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }
}

fn heap_path(db_path: &Path, table: &str) -> PathBuf {
    let parent = db_path.parent().unwrap_or(Path::new("."));
    let stem = db_path.file_stem().unwrap_or_default().to_string_lossy();
    parent.join(format!("{}.{}.heap", stem, table))
}

fn btree_path(db_path: &Path, index: &str) -> PathBuf {
    let parent = db_path.parent().unwrap_or(Path::new("."));
    let stem = db_path.file_stem().unwrap_or_default().to_string_lossy();
    parent.join(format!("{}.{}.btree", stem, index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_db(dir: &TempDir) -> Database {
        Database::open(&dir.path().join("test.db")).unwrap()
    }

    #[test]
    fn test_create_insert_select() {
        let dir = TempDir::new().unwrap();
        let mut db = make_db(&dir);
        db.execute_sql("CREATE TABLE t (id INT, name TEXT);")
            .unwrap();
        db.execute_sql("INSERT INTO t VALUES (1, 'alice');")
            .unwrap();
        db.execute_sql("INSERT INTO t VALUES (2, 'bob');").unwrap();
        let results = db.execute_sql("SELECT * FROM t;").unwrap();
        assert_eq!(results[0].rows.len(), 2);
    }

    #[test]
    fn test_where_clause() {
        let dir = TempDir::new().unwrap();
        let mut db = make_db(&dir);
        db.execute_sql("CREATE TABLE t (id INT, val INT);").unwrap();
        for i in 1..=5i64 {
            db.execute_sql(&format!("INSERT INTO t VALUES ({}, {});", i, i * 10))
                .unwrap();
        }
        let r = db.execute_sql("SELECT * FROM t WHERE val > 20;").unwrap();
        assert_eq!(r[0].rows.len(), 3);
    }

    #[test]
    fn test_update() {
        let dir = TempDir::new().unwrap();
        let mut db = make_db(&dir);
        db.execute_sql("CREATE TABLE t (id INT, name TEXT);")
            .unwrap();
        db.execute_sql("INSERT INTO t VALUES (1, 'alice');")
            .unwrap();
        let r = db
            .execute_sql("UPDATE t SET name = 'bob' WHERE id = 1;")
            .unwrap();
        assert_eq!(r[0].rows_affected, 1);
        let r2 = db.execute_sql("SELECT name FROM t WHERE id = 1;").unwrap();
        assert_eq!(r2[0].rows[0][0], Value::Text("bob".into()));
    }

    #[test]
    fn test_delete() {
        let dir = TempDir::new().unwrap();
        let mut db = make_db(&dir);
        db.execute_sql("CREATE TABLE t (id INT);").unwrap();
        db.execute_sql("INSERT INTO t VALUES (1); INSERT INTO t VALUES (2);")
            .unwrap();
        let r = db.execute_sql("DELETE FROM t WHERE id = 1;").unwrap();
        assert_eq!(r[0].rows_affected, 1);
        let r2 = db.execute_sql("SELECT * FROM t;").unwrap();
        assert_eq!(r2[0].rows.len(), 1);
    }

    #[test]
    fn test_transaction_rollback() {
        let dir = TempDir::new().unwrap();
        let mut db = make_db(&dir);
        db.execute_sql("CREATE TABLE t (id INT);").unwrap();
        db.execute_sql("BEGIN;").unwrap();
        db.execute_sql("INSERT INTO t VALUES (1);").unwrap();
        db.execute_sql("ROLLBACK;").unwrap();
        let r = db.execute_sql("SELECT * FROM t;").unwrap();
        assert_eq!(r[0].rows.len(), 0);
    }

    #[test]
    fn test_index_scan() {
        let dir = TempDir::new().unwrap();
        let mut db = make_db(&dir);
        db.execute_sql("CREATE TABLE t (id INT, val INT);").unwrap();
        db.execute_sql("CREATE INDEX idx_id ON t (id);").unwrap();
        for i in 1..=10i64 {
            db.execute_sql(&format!("INSERT INTO t VALUES ({}, {});", i, i * 2))
                .unwrap();
        }
        let r = db.execute_sql("SELECT * FROM t WHERE id = 5;").unwrap();
        assert_eq!(r[0].rows.len(), 1);
        assert_eq!(r[0].rows[0][0], Value::Int(5));
    }

    #[test]
    fn test_persistence() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("persist.db");
        {
            let mut db = Database::open(&db_path).unwrap();
            db.execute_sql("CREATE TABLE t (id INT, name TEXT);")
                .unwrap();
            db.execute_sql("INSERT INTO t VALUES (42, 'hello');")
                .unwrap();
        }
        {
            let mut db = Database::open(&db_path).unwrap();
            let r = db.execute_sql("SELECT * FROM t;").unwrap();
            assert_eq!(r[0].rows.len(), 1);
            assert_eq!(r[0].rows[0][0], Value::Int(42));
        }
    }

    #[test]
    fn test_not_null_constraint() {
        let dir = TempDir::new().unwrap();
        let mut db = make_db(&dir);
        db.execute_sql("CREATE TABLE t (id INT NOT NULL);").unwrap();
        assert!(db.execute_sql("INSERT INTO t VALUES (NULL);").is_err());
    }

    #[test]
    fn test_order_by_and_limit() {
        let dir = TempDir::new().unwrap();
        let mut db = make_db(&dir);
        db.execute_sql("CREATE TABLE t (x INT);").unwrap();
        for i in [5, 1, 3, 2, 4i64] {
            db.execute_sql(&format!("INSERT INTO t VALUES ({});", i))
                .unwrap();
        }
        let r = db
            .execute_sql("SELECT x FROM t ORDER BY x ASC LIMIT 3;")
            .unwrap();
        assert_eq!(r[0].rows.len(), 3);
        assert_eq!(r[0].rows[0][0], Value::Int(1));
        assert_eq!(r[0].rows[2][0], Value::Int(3));
    }

    #[test]
    fn test_join() {
        let dir = TempDir::new().unwrap();
        let mut db = make_db(&dir);
        db.execute_sql("CREATE TABLE users (id INT, name TEXT);")
            .unwrap();
        db.execute_sql("CREATE TABLE orders (user_id INT, amount INT);")
            .unwrap();
        db.execute_sql(
            "INSERT INTO users VALUES (1, 'alice'); INSERT INTO users VALUES (2, 'bob');",
        )
        .unwrap();
        db.execute_sql("INSERT INTO orders VALUES (1, 100); INSERT INTO orders VALUES (1, 200); INSERT INTO orders VALUES (2, 50);").unwrap();
        let r = db.execute_sql("SELECT users.name, orders.amount FROM users INNER JOIN orders ON users.id = orders.user_id WHERE users.id = 1;").unwrap();
        assert_eq!(r[0].rows.len(), 2);
    }
}

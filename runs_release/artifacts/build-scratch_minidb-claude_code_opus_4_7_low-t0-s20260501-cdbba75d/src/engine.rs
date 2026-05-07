//! Top-level database engine: opens the page file and WAL, applies any
//! committed-but-unflushed records during recovery, dispatches statements to
//! the planner/executor, and persists the in-memory state on demand.

use crate::catalog::{col_index, Catalog, IndexMeta, TableMeta};
use crate::error::{MiniDbError, Result};
use crate::exec::expr::eval;
use crate::exec::filter::Filter;
use crate::exec::index_scan::IndexScan;
use crate::exec::join::NestedLoopJoin;
use crate::exec::limit::Limit;
use crate::exec::operator::{ExecColumn, ExecSchema, PhysicalOperator, Tuple};
use crate::exec::project::Project;
use crate::exec::seq_scan::SeqScan;
use crate::exec::sort::Sort;
use crate::parser::ast::*;
use crate::plan::physical::{IndexBound, PhysicalPlan};
use crate::plan::{build_logical, build_physical};
use crate::storage::btree::BTreeIndex;
use crate::storage::heap::PageManager;
use crate::storage::wal::{wal_path_for, Wal, WalRecord};
use crate::txn::TxnManager;
use crate::types::{Column, DataType, Row, Schema, Value};
use log::{info, warn};
use std::path::Path;

/// Result of executing a statement.
#[derive(Debug, Clone)]
pub enum StmtResult {
    Rows {
        schema: ExecSchema,
        rows: Vec<Tuple>,
    },
    Mutation {
        affected: usize,
        kind: MutationKind,
    },
    Ack(String),
}

#[derive(Debug, Clone, Copy)]
pub enum MutationKind {
    Insert,
    Update,
    Delete,
    Ddl,
}

pub struct Database {
    pages: PageManager,
    wal: Wal,
    pub catalog: Catalog,
    txns: TxnManager,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let mut pages = PageManager::open(path)?;
        let snapshot = pages.read_snapshot()?;
        let catalog: Catalog = if snapshot.is_empty() {
            Catalog::new()
        } else {
            bincode::deserialize(&snapshot)?
        };
        let wal_path = wal_path_for(path);
        let wal = Wal::open(&wal_path)?;
        let mut db = Database {
            pages,
            wal,
            catalog,
            txns: TxnManager::new(),
        };
        db.recover()?;
        Ok(db)
    }

    pub fn init_empty(path: &Path) -> Result<()> {
        // Create file with empty catalog snapshot.
        let mut pages = PageManager::open(path)?;
        let cat = Catalog::new();
        let bytes = bincode::serialize(&cat)?;
        pages.write_snapshot(&bytes)?;
        // Reset WAL too.
        let mut w = Wal::open(&wal_path_for(path))?;
        w.truncate()?;
        Ok(())
    }

    fn recover(&mut self) -> Result<()> {
        let recs = self.wal.replay()?;
        if recs.is_empty() {
            return Ok(());
        }
        info!("WAL replay: {} records", recs.len());

        // Determine which transactions committed.
        use std::collections::HashSet;
        let mut committed: HashSet<u64> = HashSet::new();
        for r in &recs {
            if let WalRecord::Commit { txn_id } = r {
                committed.insert(*txn_id);
            }
        }
        // Apply only committed records (excluding Begin/Commit/Rollback themselves).
        for r in &recs {
            let Some(tid) = r.txn_id() else { continue };
            if !committed.contains(&tid) {
                continue;
            }
            self.apply_record(r)?;
        }
        // After replay, persist the snapshot and truncate WAL.
        self.flush_snapshot()?;
        warn!("recovery complete; WAL truncated");
        Ok(())
    }

    fn apply_record(&mut self, r: &WalRecord) -> Result<()> {
        match r {
            WalRecord::Begin { .. } | WalRecord::Commit { .. } | WalRecord::Rollback { .. } => {}
            WalRecord::SnapshotMarker { .. } => {}
            WalRecord::CreateTable {
                name, schema_bytes, ..
            } => {
                let schema: Schema = bincode::deserialize(schema_bytes)?;
                self.catalog.tables.insert(
                    name.to_ascii_lowercase(),
                    TableMeta {
                        name: name.clone(),
                        schema,
                        rows: Default::default(),
                    },
                );
            }
            WalRecord::DropTable { name, .. } => {
                self.catalog.tables.remove(&name.to_ascii_lowercase());
                let dead: Vec<String> = self
                    .catalog
                    .indexes
                    .iter()
                    .filter(|(_, i)| i.table.eq_ignore_ascii_case(name))
                    .map(|(k, _)| k.clone())
                    .collect();
                for k in dead {
                    self.catalog.indexes.remove(&k);
                }
            }
            WalRecord::CreateIndex {
                name,
                table,
                column,
                ..
            } => {
                let mut idx = BTreeIndex::new();
                if let Some(t) = self.catalog.get_table(table) {
                    let ci = t.schema.index_of(column).ok_or_else(|| {
                        MiniDbError::Catalog(format!("no such column {}.{}", table, column))
                    })?;
                    for (rid, row) in &t.rows {
                        idx.insert(row[ci].clone(), *rid);
                    }
                }
                self.catalog.indexes.insert(
                    name.to_ascii_lowercase(),
                    IndexMeta {
                        name: name.clone(),
                        table: table.clone(),
                        column: column.clone(),
                        btree: idx,
                    },
                );
            }
            WalRecord::DropIndex { name, .. } => {
                self.catalog.indexes.remove(&name.to_ascii_lowercase());
            }
            WalRecord::InsertRow {
                table, row_id, row, ..
            } => {
                if let Some(t) = self.catalog.get_table_mut(table) {
                    t.rows.insert(*row_id, row.clone());
                    self.catalog.next_row_id = self.catalog.next_row_id.max(*row_id + 1);
                }
                // update indexes
                let updates: Vec<(String, Value)> = self
                    .catalog
                    .indexes
                    .values()
                    .filter(|i| i.table.eq_ignore_ascii_case(table))
                    .filter_map(|i| {
                        self.catalog.get_table(table).and_then(|t| {
                            t.schema
                                .index_of(&i.column)
                                .map(|ci| (i.name.clone(), row[ci].clone()))
                        })
                    })
                    .collect();
                for (iname, key) in updates {
                    if let Some(im) = self.catalog.indexes.get_mut(&iname.to_ascii_lowercase()) {
                        im.btree.insert(key, *row_id);
                    }
                }
            }
            WalRecord::DeleteRow { table, row_id, .. } => {
                let old_row = self
                    .catalog
                    .get_table(table)
                    .and_then(|t| t.rows.get(row_id).cloned());
                if let Some(t) = self.catalog.get_table_mut(table) {
                    t.rows.remove(row_id);
                }
                if let Some(row) = old_row {
                    let names: Vec<String> = self
                        .catalog
                        .indexes
                        .values()
                        .filter(|i| i.table.eq_ignore_ascii_case(table))
                        .map(|i| i.name.clone())
                        .collect();
                    for name in names {
                        if let Some(im) = self.catalog.indexes.get_mut(&name.to_ascii_lowercase()) {
                            if let Some(t) = self.catalog.tables.get(&table.to_ascii_lowercase()) {
                                if let Some(ci) = t.schema.index_of(&im.column) {
                                    im.btree.remove(&row[ci], *row_id);
                                }
                            }
                        }
                    }
                }
            }
            WalRecord::UpdateRow {
                table,
                row_id,
                new_row,
                ..
            } => {
                let old_row = self
                    .catalog
                    .get_table(table)
                    .and_then(|t| t.rows.get(row_id).cloned());
                if let Some(t) = self.catalog.get_table_mut(table) {
                    t.rows.insert(*row_id, new_row.clone());
                }
                if let Some(old_row) = old_row {
                    let names: Vec<String> = self
                        .catalog
                        .indexes
                        .values()
                        .filter(|i| i.table.eq_ignore_ascii_case(table))
                        .map(|i| i.name.clone())
                        .collect();
                    for name in names {
                        let ci_opt = self
                            .catalog
                            .tables
                            .get(&table.to_ascii_lowercase())
                            .and_then(|t| {
                                let im = self.catalog.indexes.get(&name.to_ascii_lowercase())?;
                                t.schema.index_of(&im.column)
                            });
                        if let Some(ci) = ci_opt {
                            if let Some(im) =
                                self.catalog.indexes.get_mut(&name.to_ascii_lowercase())
                            {
                                im.btree.remove(&old_row[ci], *row_id);
                                im.btree.insert(new_row[ci].clone(), *row_id);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn flush_snapshot(&mut self) -> Result<()> {
        let bytes = bincode::serialize(&self.catalog)?;
        self.pages.write_snapshot(&bytes)?;
        self.wal.truncate()?;
        Ok(())
    }

    /// Execute a single statement; auto-commit if no explicit transaction.
    pub fn execute(&mut self, stmt: &Statement) -> Result<StmtResult> {
        match stmt {
            Statement::Begin => {
                if self.txns.is_explicit() {
                    return Err(MiniDbError::Runtime("transaction already active".into()));
                }
                let id = self.catalog.allocate_txn();
                self.txns.begin(id, true)?;
                self.wal.append(&WalRecord::Begin { txn_id: id })?;
                Ok(StmtResult::Ack("BEGIN".into()))
            }
            Statement::Commit => {
                if !self.txns.is_explicit() {
                    return Err(MiniDbError::Runtime("no active transaction".into()));
                }
                let txn = self.txns.take().unwrap();
                self.wal.append(&WalRecord::Commit { txn_id: txn.id })?;
                self.wal.sync()?;
                self.flush_snapshot()?;
                Ok(StmtResult::Ack("COMMIT".into()))
            }
            Statement::Rollback => {
                if !self.txns.is_explicit() {
                    return Err(MiniDbError::Runtime("no active transaction".into()));
                }
                let txn = self.txns.take().unwrap();
                // Revert in-memory mutations by re-loading the snapshot.
                self.wal.append(&WalRecord::Rollback { txn_id: txn.id })?;
                self.wal.sync()?;
                let snapshot = self.pages.read_snapshot()?;
                self.catalog = if snapshot.is_empty() {
                    Catalog::new()
                } else {
                    bincode::deserialize(&snapshot)?
                };
                self.wal.truncate()?;
                Ok(StmtResult::Ack("ROLLBACK".into()))
            }
            other => self.execute_in_txn(other),
        }
    }

    fn execute_in_txn(&mut self, stmt: &Statement) -> Result<StmtResult> {
        let started_implicit = !self.txns.is_active();
        if started_implicit {
            let id = self.catalog.allocate_txn();
            self.txns.begin(id, false)?;
        }
        let res = self.execute_inner(stmt);
        match res {
            Ok(r) => {
                if started_implicit {
                    let txn = self.txns.take().unwrap();
                    self.wal.append(&WalRecord::Commit { txn_id: txn.id })?;
                    self.wal.sync()?;
                    self.flush_snapshot()?;
                }
                Ok(r)
            }
            Err(e) => {
                if started_implicit {
                    // Roll back the implicit transaction by reverting to snapshot.
                    let _ = self.txns.take();
                    let snapshot = self.pages.read_snapshot()?;
                    self.catalog = if snapshot.is_empty() {
                        Catalog::new()
                    } else {
                        bincode::deserialize(&snapshot)?
                    };
                    self.wal.truncate()?;
                }
                Err(e)
            }
        }
    }

    fn execute_inner(&mut self, stmt: &Statement) -> Result<StmtResult> {
        match stmt {
            Statement::Select(sel) => {
                let logical = build_logical(sel);
                let physical = build_physical(&logical, &self.catalog)?;
                let mut op = self.build_executor(&physical)?;
                op.open()?;
                let schema = op.schema().clone();
                let mut rows = Vec::new();
                while let Some(r) = op.next()? {
                    rows.push(r);
                }
                op.close()?;
                Ok(StmtResult::Rows { schema, rows })
            }
            Statement::CreateTable {
                name,
                columns,
                checks,
            } => self.exec_create_table(name, columns, checks),
            Statement::DropTable { name } => self.exec_drop_table(name),
            Statement::CreateIndex {
                name,
                table,
                column,
            } => self.exec_create_index(name, table, column),
            Statement::DropIndex { name } => self.exec_drop_index(name),
            Statement::Insert {
                table,
                columns,
                rows,
            } => self.exec_insert(table, columns.as_deref(), rows),
            Statement::Update {
                table,
                sets,
                where_clause,
            } => self.exec_update(table, sets, where_clause.as_ref()),
            Statement::Delete {
                table,
                where_clause,
            } => self.exec_delete(table, where_clause.as_ref()),
            Statement::Begin | Statement::Commit | Statement::Rollback => unreachable!(),
        }
    }

    fn build_executor(&self, plan: &PhysicalPlan) -> Result<Box<dyn PhysicalOperator>> {
        Ok(match plan {
            PhysicalPlan::SeqScan { table, alias } => {
                let meta = self.catalog.ensure_table_exists(table)?;
                Box::new(SeqScan::new(meta, alias.as_deref()))
            }
            PhysicalPlan::IndexScan {
                table,
                alias,
                index_name,
                bound,
                ..
            } => {
                let meta = self.catalog.ensure_table_exists(table)?;
                let idx = self
                    .catalog
                    .indexes
                    .get(&index_name.to_ascii_lowercase())
                    .ok_or_else(|| {
                        MiniDbError::Catalog(format!("no such index: {}", index_name))
                    })?;
                let (lo, hi) = match bound {
                    IndexBound::Eq(e) => {
                        let v = const_eval(e)?;
                        (Some(v.clone()), Some(v))
                    }
                    IndexBound::Range { lo, hi } => (
                        match lo {
                            Some((e, _)) => Some(const_eval(e)?),
                            None => None,
                        },
                        match hi {
                            Some((e, _)) => Some(const_eval(e)?),
                            None => None,
                        },
                    ),
                };
                Box::new(IndexScan::new(meta, idx, alias.as_deref(), bound, lo, hi))
            }
            PhysicalPlan::Filter { input, predicate } => {
                Box::new(Filter::new(self.build_executor(input)?, predicate.clone()))
            }
            PhysicalPlan::Project { input, projections } => Box::new(Project::new(
                self.build_executor(input)?,
                projections.clone(),
            )),
            PhysicalPlan::NestedLoopJoin { left, right, on } => Box::new(NestedLoopJoin::new(
                self.build_executor(left)?,
                self.build_executor(right)?,
                on.clone(),
            )),
            PhysicalPlan::Sort { input, keys } => {
                Box::new(Sort::new(self.build_executor(input)?, keys.clone()))
            }
            PhysicalPlan::Limit {
                input,
                limit,
                offset,
            } => Box::new(Limit::new(self.build_executor(input)?, *limit, *offset)),
        })
    }

    fn exec_create_table(
        &mut self,
        name: &str,
        defs: &[ColumnDef],
        _checks: &[Expr],
    ) -> Result<StmtResult> {
        if self.catalog.get_table(name).is_some() {
            return Err(MiniDbError::Catalog(format!(
                "table already exists: {}",
                name
            )));
        }
        let mut cols = Vec::with_capacity(defs.len());
        for d in defs {
            let default = match &d.default {
                Some(e) => Some(const_eval(e)?),
                None => None,
            };
            cols.push(Column {
                name: d.name.clone(),
                data_type: d.data_type,
                primary_key: d.primary_key,
                not_null: d.not_null,
                unique: d.unique,
                default,
            });
        }
        let schema = Schema::new(cols);
        let txn_id = self.txns.active.as_ref().unwrap().id;
        let schema_bytes = bincode::serialize(&schema)?;
        self.wal.append(&WalRecord::CreateTable {
            txn_id,
            name: name.to_string(),
            schema_bytes,
        })?;
        self.catalog.tables.insert(
            name.to_ascii_lowercase(),
            TableMeta {
                name: name.to_string(),
                schema,
                rows: Default::default(),
            },
        );
        Ok(StmtResult::Mutation {
            affected: 0,
            kind: MutationKind::Ddl,
        })
    }

    fn exec_drop_table(&mut self, name: &str) -> Result<StmtResult> {
        if self.catalog.get_table(name).is_none() {
            return Err(MiniDbError::Catalog(format!("no such table: {}", name)));
        }
        let txn_id = self.txns.active.as_ref().unwrap().id;
        self.wal.append(&WalRecord::DropTable {
            txn_id,
            name: name.to_string(),
        })?;
        self.catalog.tables.remove(&name.to_ascii_lowercase());
        let dead: Vec<String> = self
            .catalog
            .indexes
            .iter()
            .filter(|(_, i)| i.table.eq_ignore_ascii_case(name))
            .map(|(k, _)| k.clone())
            .collect();
        for k in dead {
            self.catalog.indexes.remove(&k);
        }
        Ok(StmtResult::Mutation {
            affected: 0,
            kind: MutationKind::Ddl,
        })
    }

    fn exec_create_index(&mut self, name: &str, table: &str, column: &str) -> Result<StmtResult> {
        let table_meta = self
            .catalog
            .get_table(table)
            .ok_or_else(|| MiniDbError::Catalog(format!("no such table: {}", table)))?
            .clone();
        let ci = col_index(&table_meta, column)?;
        let mut idx = BTreeIndex::new();
        for (rid, row) in &table_meta.rows {
            idx.insert(row[ci].clone(), *rid);
        }
        let txn_id = self.txns.active.as_ref().unwrap().id;
        self.wal.append(&WalRecord::CreateIndex {
            txn_id,
            name: name.to_string(),
            table: table.to_string(),
            column: column.to_string(),
        })?;
        self.catalog.indexes.insert(
            name.to_ascii_lowercase(),
            IndexMeta {
                name: name.to_string(),
                table: table.to_string(),
                column: column.to_string(),
                btree: idx,
            },
        );
        Ok(StmtResult::Mutation {
            affected: 0,
            kind: MutationKind::Ddl,
        })
    }

    fn exec_drop_index(&mut self, name: &str) -> Result<StmtResult> {
        if !self
            .catalog
            .indexes
            .contains_key(&name.to_ascii_lowercase())
        {
            return Err(MiniDbError::Catalog(format!("no such index: {}", name)));
        }
        let txn_id = self.txns.active.as_ref().unwrap().id;
        self.wal.append(&WalRecord::DropIndex {
            txn_id,
            name: name.to_string(),
        })?;
        self.catalog.indexes.remove(&name.to_ascii_lowercase());
        Ok(StmtResult::Mutation {
            affected: 0,
            kind: MutationKind::Ddl,
        })
    }

    fn exec_insert(
        &mut self,
        table: &str,
        columns: Option<&[String]>,
        rows_in: &[Vec<Expr>],
    ) -> Result<StmtResult> {
        let meta = self
            .catalog
            .get_table(table)
            .ok_or_else(|| MiniDbError::Catalog(format!("no such table: {}", table)))?
            .clone();
        let n_cols = meta.schema.columns.len();
        let mut affected = 0usize;
        for row_exprs in rows_in {
            let mut row: Row = vec![Value::Null; n_cols];
            // populate defaults
            for (i, c) in meta.schema.columns.iter().enumerate() {
                row[i] = c.default.clone().unwrap_or(Value::Null);
            }
            // assign values
            match columns {
                Some(cs) => {
                    if cs.len() != row_exprs.len() {
                        return Err(MiniDbError::Type(
                            "column count does not match values".into(),
                        ));
                    }
                    for (cn, e) in cs.iter().zip(row_exprs.iter()) {
                        let i = col_index(&meta, cn)?;
                        let v = const_eval(e)?;
                        row[i] = v.coerce_to(meta.schema.columns[i].data_type)?;
                    }
                }
                None => {
                    if row_exprs.len() != n_cols {
                        return Err(MiniDbError::Type(format!(
                            "expected {} values, got {}",
                            n_cols,
                            row_exprs.len()
                        )));
                    }
                    for (i, e) in row_exprs.iter().enumerate() {
                        let v = const_eval(e)?;
                        row[i] = v.coerce_to(meta.schema.columns[i].data_type)?;
                    }
                }
            }
            // Constraints
            self.check_row(&meta, &row, None)?;
            let row_id = self.catalog.next_row_id();
            let txn_id = self.txns.active.as_ref().unwrap().id;
            self.wal.append(&WalRecord::InsertRow {
                txn_id,
                table: table.to_string(),
                row_id,
                row: row.clone(),
            })?;
            // apply
            if let Some(t) = self.catalog.get_table_mut(table) {
                t.rows.insert(row_id, row.clone());
            }
            // index update
            let idx_names: Vec<(String, usize)> = self
                .catalog
                .indexes
                .values()
                .filter(|i| i.table.eq_ignore_ascii_case(table))
                .filter_map(|i| {
                    meta.schema
                        .index_of(&i.column)
                        .map(|ci| (i.name.clone(), ci))
                })
                .collect();
            for (n, ci) in idx_names {
                if let Some(im) = self.catalog.indexes.get_mut(&n.to_ascii_lowercase()) {
                    im.btree.insert(row[ci].clone(), row_id);
                }
            }
            affected += 1;
        }
        Ok(StmtResult::Mutation {
            affected,
            kind: MutationKind::Insert,
        })
    }

    fn exec_update(
        &mut self,
        table: &str,
        sets: &[(String, Expr)],
        where_clause: Option<&Expr>,
    ) -> Result<StmtResult> {
        let meta = self
            .catalog
            .get_table(table)
            .ok_or_else(|| MiniDbError::Catalog(format!("no such table: {}", table)))?
            .clone();
        let exec_schema = build_table_schema(&meta, None);
        let mut updates: Vec<(u64, Row)> = Vec::new();
        for (rid, row) in &meta.rows {
            let pass = match where_clause {
                None => true,
                Some(e) => {
                    let v = eval(e, &exec_schema, row)?;
                    matches!(v, Value::Bool(true))
                }
            };
            if !pass {
                continue;
            }
            let mut new_row = row.clone();
            for (cn, e) in sets {
                let i = col_index(&meta, cn)?;
                let v = eval(e, &exec_schema, row)?;
                new_row[i] = v.coerce_to(meta.schema.columns[i].data_type)?;
            }
            self.check_row(&meta, &new_row, Some(*rid))?;
            updates.push((*rid, new_row));
        }
        let n = updates.len();
        let txn_id = self.txns.active.as_ref().unwrap().id;
        for (rid, new_row) in &updates {
            self.wal.append(&WalRecord::UpdateRow {
                txn_id,
                table: table.to_string(),
                row_id: *rid,
                new_row: new_row.clone(),
            })?;
        }
        // Apply in catalog
        let idx_pairs: Vec<(String, String)> = self
            .catalog
            .indexes
            .values()
            .filter(|i| i.table.eq_ignore_ascii_case(table))
            .map(|i| (i.name.clone(), i.column.clone()))
            .collect();
        for (rid, new_row) in updates {
            let old_row = self
                .catalog
                .get_table(table)
                .and_then(|t| t.rows.get(&rid).cloned());
            if let Some(t) = self.catalog.get_table_mut(table) {
                t.rows.insert(rid, new_row.clone());
            }
            if let Some(old) = old_row {
                for (iname, col) in &idx_pairs {
                    if let Some(ci) = meta.schema.index_of(col) {
                        if let Some(im) = self.catalog.indexes.get_mut(&iname.to_ascii_lowercase())
                        {
                            im.btree.remove(&old[ci], rid);
                            im.btree.insert(new_row[ci].clone(), rid);
                        }
                    }
                }
            }
        }
        Ok(StmtResult::Mutation {
            affected: n,
            kind: MutationKind::Update,
        })
    }

    fn exec_delete(&mut self, table: &str, where_clause: Option<&Expr>) -> Result<StmtResult> {
        let meta = self
            .catalog
            .get_table(table)
            .ok_or_else(|| MiniDbError::Catalog(format!("no such table: {}", table)))?
            .clone();
        let exec_schema = build_table_schema(&meta, None);
        let mut to_delete: Vec<u64> = Vec::new();
        for (rid, row) in &meta.rows {
            let pass = match where_clause {
                None => true,
                Some(e) => matches!(eval(e, &exec_schema, row)?, Value::Bool(true)),
            };
            if pass {
                to_delete.push(*rid);
            }
        }
        let n = to_delete.len();
        let txn_id = self.txns.active.as_ref().unwrap().id;
        for rid in &to_delete {
            self.wal.append(&WalRecord::DeleteRow {
                txn_id,
                table: table.to_string(),
                row_id: *rid,
            })?;
        }
        // apply
        let idx_pairs: Vec<(String, String)> = self
            .catalog
            .indexes
            .values()
            .filter(|i| i.table.eq_ignore_ascii_case(table))
            .map(|i| (i.name.clone(), i.column.clone()))
            .collect();
        for rid in to_delete {
            let old_row = self
                .catalog
                .get_table_mut(table)
                .and_then(|t| t.rows.remove(&rid));
            if let Some(row) = old_row {
                for (iname, col) in &idx_pairs {
                    if let Some(ci) = meta.schema.index_of(col) {
                        if let Some(im) = self.catalog.indexes.get_mut(&iname.to_ascii_lowercase())
                        {
                            im.btree.remove(&row[ci], rid);
                        }
                    }
                }
            }
        }
        Ok(StmtResult::Mutation {
            affected: n,
            kind: MutationKind::Delete,
        })
    }

    fn check_row(&self, meta: &TableMeta, row: &Row, exclude_rid: Option<u64>) -> Result<()> {
        for (i, c) in meta.schema.columns.iter().enumerate() {
            if c.not_null && row[i].is_null() {
                return Err(MiniDbError::Constraint(format!(
                    "NULL in NOT NULL column {}",
                    c.name
                )));
            }
            if c.unique && !row[i].is_null() {
                let table_meta = self.catalog.get_table(&meta.name).unwrap_or(meta);
                for (rid, other) in &table_meta.rows {
                    if Some(*rid) == exclude_rid {
                        continue;
                    }
                    if let Some(std::cmp::Ordering::Equal) = other[i].cmp_sql(&row[i]) {
                        return Err(MiniDbError::Constraint(format!(
                            "UNIQUE violation on column {}",
                            c.name
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn list_tables(&self) -> Vec<String> {
        let mut v: Vec<_> = self
            .catalog
            .tables
            .values()
            .map(|t| t.name.clone())
            .collect();
        v.sort();
        v
    }

    pub fn list_indexes(&self) -> Vec<(String, String, String)> {
        let mut v: Vec<_> = self
            .catalog
            .indexes
            .values()
            .map(|i| (i.name.clone(), i.table.clone(), i.column.clone()))
            .collect();
        v.sort();
        v
    }

    pub fn schema_of(&self, name: &str) -> Option<&Schema> {
        self.catalog.get_table(name).map(|t| &t.schema)
    }
}

/// Evaluate an expression that has no column references (used for literals and DEFAULTs).
pub fn const_eval(e: &Expr) -> Result<Value> {
    let empty = ExecSchema::empty();
    let row: Tuple = vec![];
    eval(e, &empty, &row)
}

fn build_table_schema(meta: &TableMeta, alias: Option<&str>) -> ExecSchema {
    let qual = alias.unwrap_or(&meta.name).to_string();
    ExecSchema {
        columns: meta
            .schema
            .columns
            .iter()
            .map(|c| ExecColumn {
                qualifier: Some(qual.clone()),
                name: c.name.clone(),
                data_type: Some(c.data_type),
            })
            .collect(),
    }
}

#[allow(dead_code)]
fn _silence(_d: DataType) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_sql;
    use tempfile::tempdir;

    fn open_db() -> (Database, tempfile::TempDir) {
        let d = tempdir().unwrap();
        let p = d.path().join("t.db");
        let db = Database::open(&p).unwrap();
        (db, d)
    }

    fn run(db: &mut Database, sql: &str) -> Vec<StmtResult> {
        let mut out = Vec::new();
        for s in parse_sql(sql).unwrap() {
            out.push(db.execute(&s).unwrap());
        }
        out
    }

    #[test]
    fn create_insert_select_roundtrip() {
        let (mut db, _g) = open_db();
        run(&mut db, "CREATE TABLE t(a INT PRIMARY KEY, b TEXT);");
        run(&mut db, "INSERT INTO t VALUES (1,'a'),(2,'b'),(3,'c');");
        let r = run(&mut db, "SELECT a,b FROM t WHERE a > 1 ORDER BY a;");
        match &r[0] {
            StmtResult::Rows { rows, .. } => assert_eq!(rows.len(), 2),
            _ => panic!(),
        }
    }

    #[test]
    fn rollback_reverts() {
        let (mut db, _g) = open_db();
        run(&mut db, "CREATE TABLE t(a INT);");
        run(&mut db, "BEGIN; INSERT INTO t VALUES (1); ROLLBACK;");
        let r = run(&mut db, "SELECT a FROM t;");
        match &r[0] {
            StmtResult::Rows { rows, .. } => assert_eq!(rows.len(), 0),
            _ => panic!(),
        }
    }

    #[test]
    fn unique_constraint_violation() {
        let (mut db, _g) = open_db();
        run(&mut db, "CREATE TABLE t(a INT UNIQUE);");
        run(&mut db, "INSERT INTO t VALUES (1);");
        let s = parse_sql("INSERT INTO t VALUES (1);").unwrap();
        let res = db.execute(&s[0]);
        assert!(res.is_err());
    }

    #[test]
    fn index_used_for_eq_predicate() {
        let (mut db, _g) = open_db();
        run(
            &mut db,
            "CREATE TABLE t(a INT, b INT); CREATE INDEX ix ON t(a);",
        );
        run(&mut db, "INSERT INTO t VALUES (1,10),(2,20),(3,30);");
        let stmts = parse_sql("SELECT b FROM t WHERE a = 2;").unwrap();
        let sel = match &stmts[0] {
            Statement::Select(s) => s,
            _ => panic!(),
        };
        let log = build_logical(sel);
        let phys = build_physical(&log, &db.catalog).unwrap();
        assert!(matches!(phys, PhysicalPlan::Project { .. }));
        // descend to find IndexScan
        fn has_index(p: &PhysicalPlan) -> bool {
            match p {
                PhysicalPlan::IndexScan { .. } => true,
                PhysicalPlan::Project { input, .. }
                | PhysicalPlan::Filter { input, .. }
                | PhysicalPlan::Sort { input, .. }
                | PhysicalPlan::Limit { input, .. } => has_index(input),
                PhysicalPlan::NestedLoopJoin { left, right, .. } => {
                    has_index(left) || has_index(right)
                }
                _ => false,
            }
        }
        assert!(has_index(&phys));
    }

    #[test]
    fn persistence_across_open() {
        let d = tempdir().unwrap();
        let p = d.path().join("p.db");
        {
            let mut db = Database::open(&p).unwrap();
            run(&mut db, "CREATE TABLE t(a INT); INSERT INTO t VALUES (42);");
        }
        let mut db = Database::open(&p).unwrap();
        let r = run(&mut db, "SELECT a FROM t;");
        match &r[0] {
            StmtResult::Rows { rows, .. } => {
                assert_eq!(rows.len(), 1);
                assert!(matches!(rows[0][0], Value::Int(42)));
            }
            _ => panic!(),
        }
    }
}

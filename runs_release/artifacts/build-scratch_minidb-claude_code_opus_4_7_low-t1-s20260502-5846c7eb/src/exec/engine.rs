//! Top-level database engine: ties together storage, catalog, transactions
//! and the executor. Exposes `Database::open` and `Database::execute_sql`.

use crate::catalog::{
    catalog_path, load_catalog, save_catalog, Catalog, ColumnSchema, IndexSchema, TableSchema,
};
use crate::error::{MiniDbError, Result};
use crate::exec::expr::{eval, predicate_truth};
use crate::exec::operators::{self, schema_for_table, ExecCtx};
use crate::parser::ast::{self, parse_expr_string, Statement};
use crate::parser::Parser;
use crate::plan::logical::plan_select;
use crate::plan::physical::build_physical;
use crate::plan::physical::Schema;
use crate::storage::btree::{index_path, load_index, save_index, BTreeIndex};
use crate::storage::heap::{delete_slot, heap_insert, num_slots, HeapIter, RowId};
use crate::storage::page::{decode_tuple, encode_tuple};
use crate::storage::pager::Pager;
use crate::storage::wal::{committed_records, Wal, WalRecord};
use crate::txn::TransactionManager;
use crate::types::{Row, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Output of executing a single SQL statement.
#[derive(Debug, Clone)]
pub enum StatementResult {
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
    Affected {
        kind: AffectedKind,
        n: usize,
    },
    Acknowledged(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AffectedKind {
    Inserted,
    Updated,
    Deleted,
}

impl AffectedKind {
    pub fn verb(self) -> &'static str {
        match self {
            AffectedKind::Inserted => "inserted",
            AffectedKind::Updated => "affected",
            AffectedKind::Deleted => "deleted",
        }
    }
}

pub struct Database {
    pub path: PathBuf,
    pub pager: Pager,
    pub wal: Wal,
    pub catalog: Catalog,
    pub indexes: HashMap<String, BTreeIndex>,
    pub txn: TransactionManager,
}

fn wal_path(db_path: &Path) -> PathBuf {
    let mut p = db_path.to_path_buf();
    let stem = p
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "minidb".to_string());
    p.set_file_name(format!("{}.wal", stem));
    p
}

impl Database {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let pager = Pager::open(&path)?;
        let wal = Wal::open(wal_path(&path))?;
        let mut catalog = load_catalog(&catalog_path(&path))?;
        // re-parse stored expressions
        for t in catalog.tables.values_mut() {
            t.checks.clear();
            for blob in &t.checks_blob {
                let s = std::str::from_utf8(blob)
                    .map_err(|_| MiniDbError::Catalog("non-utf8 check".into()))?;
                t.checks.push(parse_expr_string(s)?);
            }
            for c in t.columns.iter_mut() {
                if let Some(blob) = &c.default_blob {
                    let s = std::str::from_utf8(blob)
                        .map_err(|_| MiniDbError::Catalog("non-utf8 default".into()))?;
                    c.default_expr = Some(parse_expr_string(s)?);
                }
            }
        }

        // Load indexes
        let mut indexes = HashMap::new();
        for idx in catalog.indexes.values() {
            let p = index_path(&path, &idx.name);
            indexes.insert(idx.name.clone(), load_index(&p)?);
        }

        let mut db = Database {
            path,
            pager,
            wal,
            catalog,
            indexes,
            txn: TransactionManager::new(),
        };

        db.replay_wal()?;
        Ok(db)
    }

    fn replay_wal(&mut self) -> Result<()> {
        let recs = self.wal.read_all()?;
        if recs.is_empty() {
            return Ok(());
        }
        let committed = committed_records(recs);
        if committed.is_empty() {
            self.wal.truncate()?;
            return Ok(());
        }
        log::info!("replaying {} WAL record(s)", committed.len());
        for r in committed {
            match r {
                WalRecord::Insert {
                    table,
                    page,
                    slot,
                    tuple,
                    ..
                } => {
                    self.replay_write(&table, page, slot, &tuple)?;
                }
                WalRecord::Delete {
                    table, page, slot, ..
                } => {
                    self.replay_delete(&table, page, slot)?;
                }
                WalRecord::Update {
                    table,
                    page,
                    slot,
                    new,
                    ..
                } => {
                    self.replay_write(&table, page, slot, &new)?;
                }
                WalRecord::CreateTable { schema_blob, .. } => {
                    let s = std::str::from_utf8(&schema_blob)
                        .map_err(|_| MiniDbError::Wal("invalid utf8".into()))?
                        .to_string();
                    let stmts = Parser::parse_sql(&s)?;
                    for st in stmts {
                        if let Statement::CreateTable(ct) = st {
                            if !self.catalog.tables.contains_key(&ct.name) {
                                self.create_table_internal(ct, false)?;
                            }
                        }
                    }
                }
                WalRecord::DropTable { table, .. } => {
                    let _ = self.catalog.drop_table(&table);
                }
                WalRecord::CreateIndex { blob, .. } => {
                    let s = std::str::from_utf8(&blob)
                        .map_err(|_| MiniDbError::Wal("invalid utf8".into()))?
                        .to_string();
                    let stmts = Parser::parse_sql(&s)?;
                    for st in stmts {
                        if let Statement::CreateIndex {
                            name,
                            table,
                            column,
                        } = st
                        {
                            if !self.catalog.indexes.contains_key(&name) {
                                self.create_index_internal(name, table, column, false)?;
                            }
                        }
                    }
                }
                WalRecord::DropIndex { name, .. } => {
                    let _ = self.catalog.drop_index(&name);
                    self.indexes.remove(&name);
                }
                _ => {}
            }
        }
        // Persist replayed state
        self.checkpoint()?;
        Ok(())
    }

    fn replay_write(&mut self, table: &str, page: u64, slot: u16, tuple: &[u8]) -> Result<()> {
        // Place the tuple at exactly (page, slot). Allocate pages as needed.
        let pid = crate::storage::page::PageId(page);
        // ensure page exists
        while self.pager.next_page_id() <= page {
            let _ = self.pager.allocate_page()?;
        }
        let pg = self.pager.page_mut(pid)?;
        // ensure header initialised
        if num_slots(pg) == 0 {
            let s_off = crate::storage::PAGE_SIZE - (slot as usize + 1) * 4;
            // basic init if page is zeroed
            if pg.data.iter().all(|b| *b == 0) {
                crate::storage::heap::init_heap_page(pg);
            }
            let _ = s_off;
        }
        // Write tuple at next free position (best effort) and record slot mapping.
        let offset = {
            let pg2 = self.pager.page_mut(pid)?;
            let fs_pos = 10;
            let free_start = u16::from_le_bytes([pg2.data[fs_pos], pg2.data[fs_pos + 1]]) as usize;
            let len = tuple.len();
            pg2.data[free_start..free_start + len].copy_from_slice(tuple);
            let new_fs = free_start + len;
            pg2.data[fs_pos..fs_pos + 2].copy_from_slice(&(new_fs as u16).to_le_bytes());
            free_start
        };
        // Write slot entry at PAGE_SIZE - (slot+1)*4
        let pg3 = self.pager.page_mut(pid)?;
        let so = crate::storage::PAGE_SIZE - (slot as usize + 1) * 4;
        pg3.data[so..so + 2].copy_from_slice(&(offset as u16).to_le_bytes());
        pg3.data[so + 2..so + 4].copy_from_slice(&(tuple.len() as u16).to_le_bytes());
        // Bump num_slots / free_end as needed
        let cur_slots = u16::from_le_bytes([pg3.data[8], pg3.data[9]]);
        if slot + 1 > cur_slots {
            pg3.data[8..10].copy_from_slice(&(slot + 1).to_le_bytes());
        }
        let cur_fe = u16::from_le_bytes([pg3.data[12], pg3.data[13]]) as usize;
        if so < cur_fe {
            pg3.data[12..14].copy_from_slice(&(so as u16).to_le_bytes());
        }
        // update catalog first_page if not set
        if let Some(t) = self.catalog.tables.get_mut(table) {
            if t.first_page.is_none() {
                t.first_page = Some(pid);
            }
        }
        // also: rebuild affected indexes lazily — handled at end-of-replay rebuild
        Ok(())
    }

    fn replay_delete(&mut self, _table: &str, page: u64, slot: u16) -> Result<()> {
        let pid = crate::storage::page::PageId(page);
        if pid.raw() < self.pager.next_page_id() {
            let pg = self.pager.page_mut(pid)?;
            delete_slot(pg, slot);
        }
        Ok(())
    }

    fn checkpoint(&mut self) -> Result<()> {
        self.pager.flush()?;
        save_catalog(&catalog_path(&self.path), &self.catalog)?;
        for (name, idx) in &self.indexes {
            save_index(&index_path(&self.path, name), idx)?;
        }
        // Rebuild indexes from data after replay (fixes any drift)
        let to_rebuild: Vec<String> = self.catalog.indexes.keys().cloned().collect();
        for name in to_rebuild {
            self.rebuild_index(&name)?;
        }
        for (name, idx) in &self.indexes {
            save_index(&index_path(&self.path, name), idx)?;
        }
        self.wal.truncate()?;
        Ok(())
    }

    fn rebuild_index(&mut self, name: &str) -> Result<()> {
        let info = self
            .catalog
            .indexes
            .get(name)
            .ok_or_else(|| MiniDbError::NoSuchIndex(name.into()))?
            .clone();
        let t = self.catalog.get_table(&info.table)?.clone();
        let col_idx = t
            .column_index(&info.column)
            .ok_or_else(|| MiniDbError::NoSuchColumn(info.column.clone()))?;
        let types = t.data_types();
        let mut idx = BTreeIndex::new();
        let mut iter = HeapIter::new(&mut self.pager, t.first_page);
        while let Some((rid, bytes)) = iter.next() {
            let vals = decode_tuple(&bytes)?;
            let coerced: Result<Vec<Value>> = vals
                .into_iter()
                .zip(types.iter())
                .map(|(v, t)| v.coerce(*t))
                .collect();
            let row = coerced?;
            idx.insert(row[col_idx].clone(), rid);
        }
        self.indexes.insert(name.to_string(), idx);
        Ok(())
    }

    pub fn execute_sql(&mut self, sql: &str) -> Result<Vec<StatementResult>> {
        let stmts = Parser::parse_sql(sql)?;
        let mut out = Vec::new();
        for s in stmts {
            out.push(self.execute_statement(s)?);
        }
        Ok(out)
    }

    pub fn execute_statement(&mut self, stmt: Statement) -> Result<StatementResult> {
        // Auto-begin a transaction for DML/DDL when none is active.
        let needs_txn = match &stmt {
            Statement::Begin | Statement::Commit | Statement::Rollback | Statement::Select(_) => {
                false
            }
            _ => true,
        };
        let auto = needs_txn && !self.txn.in_transaction();
        if auto {
            self.txn.begin(
                false,
                &self.catalog,
                &self.indexes,
                self.pager.next_page_id(),
            )?;
        }
        let res = self.exec_inner(stmt);
        match (auto, res) {
            (true, Ok(r)) => {
                self.commit_internal()?;
                Ok(r)
            }
            (true, Err(e)) => {
                self.rollback_internal()?;
                Err(e)
            }
            (false, r) => r,
        }
    }

    fn exec_inner(&mut self, stmt: Statement) -> Result<StatementResult> {
        match stmt {
            Statement::Begin => {
                if self.txn.in_transaction() {
                    return Err(MiniDbError::Runtime("already in a transaction".into()));
                }
                self.txn.begin(
                    true,
                    &self.catalog,
                    &self.indexes,
                    self.pager.next_page_id(),
                )?;
                Ok(StatementResult::Acknowledged("begin".into()))
            }
            Statement::Commit => {
                if !self.txn.in_transaction() {
                    return Err(MiniDbError::Runtime("no transaction to commit".into()));
                }
                self.commit_internal()?;
                Ok(StatementResult::Acknowledged("commit".into()))
            }
            Statement::Rollback => {
                if !self.txn.in_transaction() {
                    return Err(MiniDbError::Runtime("no transaction to rollback".into()));
                }
                self.rollback_internal()?;
                Ok(StatementResult::Acknowledged("rollback".into()))
            }
            Statement::CreateTable(ct) => {
                let txn_id = self.txn.current().map(|t| t.id).unwrap_or(0);
                let sql = ct_to_sql(&ct);
                let exists = self.catalog.tables.contains_key(&ct.name);
                if exists && ct.if_not_exists {
                    return Ok(StatementResult::Acknowledged("ok".into()));
                }
                self.snapshot_catalog_for_txn();
                self.create_table_internal(ct, true)?;
                self.txn.record(WalRecord::CreateTable {
                    txn_id,
                    schema_blob: sql.into_bytes(),
                });
                Ok(StatementResult::Acknowledged("ok".into()))
            }
            Statement::DropTable { name, if_exists } => {
                if !self.catalog.tables.contains_key(&name) {
                    if if_exists {
                        return Ok(StatementResult::Acknowledged("ok".into()));
                    }
                    return Err(MiniDbError::NoSuchTable(name));
                }
                let txn_id = self.txn.current().map(|t| t.id).unwrap_or(0);
                self.snapshot_catalog_for_txn();
                let dropped_indexes: Vec<String> = self
                    .catalog
                    .indexes_for(&name)
                    .iter()
                    .map(|i| i.name.clone())
                    .collect();
                self.catalog.drop_table(&name)?;
                for n in dropped_indexes {
                    self.indexes.remove(&n);
                }
                self.txn.record(WalRecord::DropTable {
                    txn_id,
                    table: name,
                });
                Ok(StatementResult::Acknowledged("ok".into()))
            }
            Statement::CreateIndex {
                name,
                table,
                column,
            } => {
                let txn_id = self.txn.current().map(|t| t.id).unwrap_or(0);
                self.snapshot_catalog_for_txn();
                let blob = format!("CREATE INDEX {} ON {} ({});", name, table, column);
                self.create_index_internal(name, table, column, true)?;
                self.txn.record(WalRecord::CreateIndex {
                    txn_id,
                    blob: blob.into_bytes(),
                });
                Ok(StatementResult::Acknowledged("ok".into()))
            }
            Statement::DropIndex { name, if_exists } => {
                if !self.catalog.indexes.contains_key(&name) {
                    if if_exists {
                        return Ok(StatementResult::Acknowledged("ok".into()));
                    }
                    return Err(MiniDbError::NoSuchIndex(name));
                }
                let txn_id = self.txn.current().map(|t| t.id).unwrap_or(0);
                self.snapshot_catalog_for_txn();
                self.catalog.drop_index(&name)?;
                self.indexes.remove(&name);
                self.txn.record(WalRecord::DropIndex { txn_id, name });
                Ok(StatementResult::Acknowledged("ok".into()))
            }
            Statement::Insert(ins) => self.exec_insert(ins),
            Statement::Update(u) => self.exec_update(u),
            Statement::Delete(d) => self.exec_delete(d),
            Statement::Select(s) => self.exec_select(*s),
        }
    }

    fn snapshot_catalog_for_txn(&mut self) {
        if let Some(t) = self.txn.current_mut() {
            if t.before_catalog.is_none() {
                t.before_catalog = Some(self.catalog.clone());
            }
        }
    }

    fn create_table_internal(
        &mut self,
        ct: ast::CreateTable,
        record_default_blobs: bool,
    ) -> Result<()> {
        if self.catalog.tables.contains_key(&ct.name) {
            return Err(MiniDbError::AlreadyExists(format!("table {}", ct.name)));
        }
        let mut cols = Vec::with_capacity(ct.columns.len());
        for c in &ct.columns {
            let default_blob = if record_default_blobs {
                c.default.as_ref().map(|e| format!("{}", e).into_bytes())
            } else {
                c.default.as_ref().map(|e| format!("{}", e).into_bytes())
            };
            cols.push(ColumnSchema {
                name: c.name.clone(),
                data_type: c.data_type,
                primary_key: c.primary_key,
                not_null: c.not_null,
                unique: c.unique,
                default_blob,
                default_expr: c.default.clone(),
            });
        }
        let checks_blob: Vec<Vec<u8>> = ct
            .checks
            .iter()
            .map(|e| format!("{}", e).into_bytes())
            .collect();
        let schema = TableSchema {
            name: ct.name.clone(),
            columns: cols,
            first_page: None,
            checks_blob,
            checks: ct.checks.clone(),
        };
        self.catalog.add_table(schema)?;
        // Auto-create implicit unique-enforcement index for PK column.
        let pk_col = ct
            .columns
            .iter()
            .find(|c| c.primary_key)
            .map(|c| c.name.clone());
        if let Some(pk) = pk_col {
            let idx_name = format!("__pk_{}", ct.name);
            self.create_index_internal(idx_name, ct.name.clone(), pk, false)?;
        }
        // also unique columns
        for c in &ct.columns {
            if c.unique && !c.primary_key {
                let idx_name = format!("__uq_{}_{}", ct.name, c.name);
                self.create_index_internal(idx_name, ct.name.clone(), c.name.clone(), false)?;
            }
        }
        Ok(())
    }

    fn create_index_internal(
        &mut self,
        name: String,
        table: String,
        column: String,
        check_dup: bool,
    ) -> Result<()> {
        if self.catalog.indexes.contains_key(&name) {
            if check_dup {
                return Err(MiniDbError::AlreadyExists(format!("index {}", name)));
            }
            return Ok(());
        }
        self.catalog.add_index(IndexSchema {
            name: name.clone(),
            table: table.clone(),
            column: column.clone(),
        })?;
        self.indexes.insert(name.clone(), BTreeIndex::new());
        // Populate from existing data
        let t = self.catalog.get_table(&table)?.clone();
        let col_idx = t
            .column_index(&column)
            .ok_or_else(|| MiniDbError::NoSuchColumn(column.clone()))?;
        let types = t.data_types();
        let first = t.first_page;
        let mut entries: Vec<(Value, RowId)> = Vec::new();
        {
            let mut iter = HeapIter::new(&mut self.pager, first);
            while let Some((rid, bytes)) = iter.next() {
                let vals = decode_tuple(&bytes)?;
                let coerced: Result<Vec<Value>> = vals
                    .into_iter()
                    .zip(types.iter())
                    .map(|(v, t)| v.coerce(*t))
                    .collect();
                let row = coerced?;
                entries.push((row[col_idx].clone(), rid));
            }
        }
        let idx_ref = self.indexes.get_mut(&name).unwrap();
        for (k, rid) in entries {
            idx_ref.insert(k, rid);
        }
        Ok(())
    }

    fn exec_insert(&mut self, ins: ast::Insert) -> Result<StatementResult> {
        let table = self.catalog.get_table(&ins.table)?.clone();
        let col_indices: Vec<usize> = match &ins.columns {
            None => (0..table.columns.len()).collect(),
            Some(names) => {
                let mut out = Vec::with_capacity(names.len());
                for n in names {
                    let i = table
                        .column_index(n)
                        .ok_or_else(|| MiniDbError::NoSuchColumn(n.clone()))?;
                    out.push(i);
                }
                out
            }
        };
        let provided_set: std::collections::HashSet<usize> = col_indices.iter().copied().collect();
        let mut total = 0usize;
        for row_exprs in &ins.rows {
            if row_exprs.len() != col_indices.len() {
                return Err(MiniDbError::Type(format!(
                    "INSERT into {} expects {} values, got {}",
                    ins.table,
                    col_indices.len(),
                    row_exprs.len()
                )));
            }
            // Build full row in column order
            let mut row_vals: Vec<Value> = (0..table.columns.len()).map(|_| Value::Null).collect();
            let dummy_schema = Schema::empty();
            let dummy_row = Row::new(vec![]);
            for (i, e) in row_exprs.iter().enumerate() {
                let idx = col_indices[i];
                let v = eval(e, &dummy_row, &dummy_schema)?;
                row_vals[idx] = v.coerce(table.columns[idx].data_type)?;
            }
            for (i, c) in table.columns.iter().enumerate() {
                if !provided_set.contains(&i) {
                    if let Some(de) = &c.default_expr {
                        let v = eval(de, &dummy_row, &dummy_schema)?;
                        row_vals[i] = v.coerce(c.data_type)?;
                    }
                }
            }
            self.insert_row(&ins.table, row_vals)?;
            total += 1;
        }
        Ok(StatementResult::Affected {
            kind: AffectedKind::Inserted,
            n: total,
        })
    }

    fn insert_row(&mut self, table: &str, values: Vec<Value>) -> Result<()> {
        let t = self.catalog.get_table(table)?.clone();
        // Constraint checks
        for (i, c) in t.columns.iter().enumerate() {
            if c.not_null && matches!(values[i], Value::Null) {
                return Err(MiniDbError::Constraint(format!(
                    "NOT NULL constraint failed: {}.{}",
                    table, c.name
                )));
            }
        }
        // CHECK constraints
        let row = Row::new(values.clone());
        let schema = schema_for_table(table, &t);
        for chk in &t.checks {
            let v = eval(chk, &row, &schema)?;
            if !matches!(predicate_truth(&v), crate::types::TriBool::True) {
                return Err(MiniDbError::Constraint(format!(
                    "CHECK constraint failed on {}",
                    table
                )));
            }
        }
        // UNIQUE checks via existing indexes
        for idx_meta in self.catalog.indexes_for(table) {
            let idx_meta = idx_meta.clone();
            if idx_meta.name.starts_with("__pk_") || idx_meta.name.starts_with("__uq_") {
                let col_i = t.column_index(&idx_meta.column).unwrap();
                let v = &values[col_i];
                if !matches!(v, Value::Null) {
                    let idx = self.indexes.get(&idx_meta.name).unwrap();
                    if !idx.equal_range(v).is_empty() {
                        return Err(MiniDbError::Constraint(format!(
                            "UNIQUE constraint failed: {}.{}",
                            table, idx_meta.column
                        )));
                    }
                }
            }
        }
        // Encode and insert
        let bytes = encode_tuple(&values);
        let first = self.catalog.get_table(table)?.first_page;
        let (rid, new_first) = heap_insert(&mut self.pager, first, &bytes)?;
        if first.is_none() {
            self.catalog.get_table_mut(table).unwrap().first_page = Some(new_first);
        }
        // Update indexes
        let indexes_for: Vec<IndexSchema> = self
            .catalog
            .indexes_for(table)
            .into_iter()
            .cloned()
            .collect();
        for idx_meta in indexes_for {
            let col_i = t.column_index(&idx_meta.column).unwrap();
            let idx = self.indexes.get_mut(&idx_meta.name).unwrap();
            idx.insert(values[col_i].clone(), rid);
        }
        let txn_id = self.txn.current().map(|t| t.id).unwrap_or(0);
        self.txn.record(WalRecord::Insert {
            txn_id,
            table: table.to_string(),
            page: rid.page.raw(),
            slot: rid.slot,
            tuple: bytes,
        });
        Ok(())
    }

    fn exec_delete(&mut self, d: ast::Delete) -> Result<StatementResult> {
        let t = self.catalog.get_table(&d.table)?.clone();
        let schema = schema_for_table(&d.table, &t);
        let types = t.data_types();
        let mut victims: Vec<(RowId, Vec<Value>)> = Vec::new();
        {
            let mut iter = HeapIter::new(&mut self.pager, t.first_page);
            while let Some((rid, bytes)) = iter.next() {
                let vals = decode_tuple(&bytes)?;
                let coerced: Result<Vec<Value>> = vals
                    .into_iter()
                    .zip(types.iter())
                    .map(|(v, t)| v.coerce(*t))
                    .collect();
                let row = coerced?;
                if let Some(pred) = &d.where_clause {
                    let v = eval(pred, &Row::new(row.clone()), &schema)?;
                    if !predicate_truth(&v).is_true() {
                        continue;
                    }
                }
                victims.push((rid, row));
            }
        }
        for (rid, row) in &victims {
            let pg = self.pager.page_mut(rid.page)?;
            delete_slot(pg, rid.slot);
            // remove from indexes
            let indexes_for: Vec<IndexSchema> = self
                .catalog
                .indexes_for(&d.table)
                .into_iter()
                .cloned()
                .collect();
            for idx_meta in indexes_for {
                let col_i = t.column_index(&idx_meta.column).unwrap();
                let idx = self.indexes.get_mut(&idx_meta.name).unwrap();
                idx.remove(&row[col_i], *rid);
            }
            let txn_id = self.txn.current().map(|t| t.id).unwrap_or(0);
            self.txn.record(WalRecord::Delete {
                txn_id,
                table: d.table.clone(),
                page: rid.page.raw(),
                slot: rid.slot,
            });
        }
        Ok(StatementResult::Affected {
            kind: AffectedKind::Deleted,
            n: victims.len(),
        })
    }

    fn exec_update(&mut self, u: ast::Update) -> Result<StatementResult> {
        let t = self.catalog.get_table(&u.table)?.clone();
        let schema = schema_for_table(&u.table, &t);
        let types = t.data_types();
        // Resolve assignment indexes
        let mut assigns = Vec::new();
        for (col, expr) in &u.assignments {
            let i = t
                .column_index(col)
                .ok_or_else(|| MiniDbError::NoSuchColumn(col.clone()))?;
            assigns.push((i, expr.clone()));
        }
        let mut targets: Vec<(RowId, Vec<Value>)> = Vec::new();
        {
            let mut iter = HeapIter::new(&mut self.pager, t.first_page);
            while let Some((rid, bytes)) = iter.next() {
                let vals = decode_tuple(&bytes)?;
                let coerced: Result<Vec<Value>> = vals
                    .into_iter()
                    .zip(types.iter())
                    .map(|(v, t)| v.coerce(*t))
                    .collect();
                let row = coerced?;
                if let Some(pred) = &u.where_clause {
                    let v = eval(pred, &Row::new(row.clone()), &schema)?;
                    if !predicate_truth(&v).is_true() {
                        continue;
                    }
                }
                targets.push((rid, row));
            }
        }
        let mut count = 0;
        for (rid, old) in &targets {
            let mut new_row = old.clone();
            let row_for_eval = Row::new(old.clone());
            for (i, e) in &assigns {
                let v = eval(e, &row_for_eval, &schema)?;
                new_row[*i] = v.coerce(t.columns[*i].data_type)?;
            }
            // Constraint checks
            for (i, c) in t.columns.iter().enumerate() {
                if c.not_null && matches!(new_row[i], Value::Null) {
                    return Err(MiniDbError::Constraint(format!(
                        "NOT NULL constraint failed: {}.{}",
                        u.table, c.name
                    )));
                }
            }
            for chk in &t.checks {
                let v = eval(chk, &Row::new(new_row.clone()), &schema)?;
                if !matches!(predicate_truth(&v), crate::types::TriBool::True) {
                    return Err(MiniDbError::Constraint(format!(
                        "CHECK constraint failed on {}",
                        u.table
                    )));
                }
            }
            // delete + insert
            let pg = self.pager.page_mut(rid.page)?;
            delete_slot(pg, rid.slot);
            // remove from indexes
            let indexes_for: Vec<IndexSchema> = self
                .catalog
                .indexes_for(&u.table)
                .into_iter()
                .cloned()
                .collect();
            for idx_meta in &indexes_for {
                let col_i = t.column_index(&idx_meta.column).unwrap();
                let idx = self.indexes.get_mut(&idx_meta.name).unwrap();
                idx.remove(&old[col_i], *rid);
            }
            // unique check among committed rows + this txn's prior new rows is approximate; we re-check via index after partial removal
            for idx_meta in &indexes_for {
                if idx_meta.name.starts_with("__pk_") || idx_meta.name.starts_with("__uq_") {
                    let col_i = t.column_index(&idx_meta.column).unwrap();
                    let v = &new_row[col_i];
                    if !matches!(v, Value::Null) {
                        let idx = self.indexes.get(&idx_meta.name).unwrap();
                        if !idx.equal_range(v).is_empty() {
                            return Err(MiniDbError::Constraint(format!(
                                "UNIQUE constraint failed: {}.{}",
                                u.table, idx_meta.column
                            )));
                        }
                    }
                }
            }
            let bytes = encode_tuple(&new_row);
            let first = self.catalog.get_table(&u.table)?.first_page;
            let (new_rid, new_first) = heap_insert(&mut self.pager, first, &bytes)?;
            if first.is_none() {
                self.catalog.get_table_mut(&u.table).unwrap().first_page = Some(new_first);
            }
            for idx_meta in &indexes_for {
                let col_i = t.column_index(&idx_meta.column).unwrap();
                let idx = self.indexes.get_mut(&idx_meta.name).unwrap();
                idx.insert(new_row[col_i].clone(), new_rid);
            }
            let txn_id = self.txn.current().map(|t| t.id).unwrap_or(0);
            self.txn.record(WalRecord::Delete {
                txn_id,
                table: u.table.clone(),
                page: rid.page.raw(),
                slot: rid.slot,
            });
            self.txn.record(WalRecord::Insert {
                txn_id,
                table: u.table.clone(),
                page: new_rid.page.raw(),
                slot: new_rid.slot,
                tuple: bytes,
            });
            count += 1;
        }
        Ok(StatementResult::Affected {
            kind: AffectedKind::Updated,
            n: count,
        })
    }

    fn exec_select(&mut self, s: ast::Select) -> Result<StatementResult> {
        let logical = plan_select(&s)?;
        let physical = build_physical(&self.catalog, logical)?;
        let columns: Vec<String> = physical.schema().names();
        let mut tree = operators::build(physical);
        let mut ctx = ExecCtx {
            pager: &mut self.pager,
            catalog: &self.catalog,
            indexes: &self.indexes,
        };
        tree.open(&mut ctx)?;
        let mut rows = Vec::new();
        while let Some(r) = tree.next(&mut ctx)? {
            rows.push(r.0);
        }
        tree.close(&mut ctx)?;
        Ok(StatementResult::Rows { columns, rows })
    }

    fn commit_internal(&mut self) -> Result<()> {
        let txn = self.txn.take().expect("txn active");
        // Skip empty txns (no records beyond Begin) — still treat as committed.
        for r in &txn.records {
            self.wal.append(r)?;
        }
        self.wal.append(&WalRecord::Commit { txn_id: txn.id })?;
        self.wal.fsync()?;
        // Flush pager + catalog + indexes
        self.pager.flush()?;
        save_catalog(&catalog_path(&self.path), &self.catalog)?;
        for (name, idx) in &self.indexes {
            save_index(&index_path(&self.path, name), idx)?;
        }
        // Truncate WAL after successful checkpoint
        self.wal.truncate()?;
        Ok(())
    }

    fn rollback_internal(&mut self) -> Result<()> {
        let txn = self.txn.take().expect("txn active");
        if let Some(cat) = txn.before_catalog {
            self.catalog = cat;
        }
        self.indexes = txn.before_indexes;
        // Discard cached pager state — uncommitted pages are not on disk, so
        // future reads will pull the pre-transaction state from the file.
        self.pager.discard_cache();
        self.pager.reset_next_page(txn.before_next_page);
        Ok(())
    }

    pub fn close(mut self) -> Result<()> {
        // Best-effort flush
        self.pager.flush()?;
        save_catalog(&catalog_path(&self.path), &self.catalog)?;
        for (name, idx) in &self.indexes {
            save_index(&index_path(&self.path, name), idx)?;
        }
        Ok(())
    }
}

/// Re-emit a CREATE TABLE statement as SQL for WAL persistence.
fn ct_to_sql(ct: &ast::CreateTable) -> String {
    let mut s = format!("CREATE TABLE {} (", ct.name);
    let mut parts = Vec::new();
    for c in &ct.columns {
        let mut p = format!("{} {}", c.name, c.data_type);
        if c.primary_key {
            p.push_str(" PRIMARY KEY");
        } else if c.not_null {
            p.push_str(" NOT NULL");
        }
        if c.unique && !c.primary_key {
            p.push_str(" UNIQUE");
        }
        if let Some(d) = &c.default {
            p.push_str(&format!(" DEFAULT {}", d));
        }
        parts.push(p);
    }
    for chk in &ct.checks {
        parts.push(format!("CHECK ({})", chk));
    }
    s.push_str(&parts.join(", "));
    s.push_str(");");
    s
}

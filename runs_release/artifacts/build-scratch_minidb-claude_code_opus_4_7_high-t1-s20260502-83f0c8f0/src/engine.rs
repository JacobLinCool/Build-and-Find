//! Database engine: ties together storage, catalog, and the transaction
//! manager. Provides the read/write API consumed by the executor.
//!
//! ## Persistence model
//!
//! - The pager owns one data file. Page 0 is the header, holding (among
//!   other things) the catalog root page id.
//! - The catalog is serialised into a chain of pages on every DDL change.
//! - DDL statements always auto-commit their own micro-transaction (so the
//!   catalog and the on-disk heap stay consistent without needing
//!   shadow-pages for DDL).
//! - DML statements buffer pending operations in memory under an explicit
//!   `BEGIN`. On `COMMIT`, the WAL is written and fsynced first, then the
//!   buffered operations are applied to the data file, then the WAL is
//!   truncated. On `ROLLBACK`, the buffer is discarded and the WAL is
//!   truncated.
//! - On startup, any committed WAL records are replayed (forward) and the
//!   WAL is truncated; uncommitted records are dropped.

use crate::catalog::{Catalog, Index, Table};
use crate::error::{MiniDbError, Result};
use crate::storage::btree::{BTree, IndexEntry};
use crate::storage::heap::Heap;
use crate::storage::page::{PageId, RowId, PAGE_SIZE};
use crate::storage::pager::Pager;
use crate::storage::wal::{self, WalRecord, WalWriter};
use crate::txn::{DdlOp, TxnBuffer, TxnManager};
use crate::types::{ColumnDef, DataType, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A row tagged with either a real row id (committed in the heap) or with
/// the index of a buffered pending insert.
#[derive(Debug, Clone)]
pub enum RowRef {
    /// Row already on disk, identified by RowId.
    Persistent(RowId),
    /// Row buffered for insertion in the open transaction.
    Pending(usize),
}

#[derive(Debug, Clone)]
pub struct VisibleRow {
    pub rref: RowRef,
    pub values: Vec<Value>,
}

pub struct Engine {
    pub(crate) pager: Pager,
    pub(crate) wal: WalWriter,
    pub(crate) wal_path: PathBuf,
    pub(crate) db_path: PathBuf,
    pub(crate) catalog: Catalog,
    pub(crate) txn: TxnManager,
    pub(crate) buffer: TxnBuffer,
}

impl Engine {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db_path: PathBuf = path.as_ref().into();
        let pager = Pager::open(&db_path)?;
        let wal_path = wal_path_for(&db_path);
        let wal = WalWriter::open(&wal_path)?;

        let mut engine = Engine {
            pager,
            wal,
            wal_path: wal_path.clone(),
            db_path: db_path.clone(),
            catalog: Catalog::new(),
            txn: TxnManager::new(),
            buffer: TxnBuffer::new(),
        };

        engine.load_catalog()?;
        engine.recover()?;
        Ok(engine)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    fn load_catalog(&mut self) -> Result<()> {
        let root = self.pager.header().catalog_root;
        if root == 0 {
            return Ok(());
        }
        let bytes = read_catalog_blob(&mut self.pager, PageId(root))?;
        let cat = decode_catalog(&bytes)?;
        self.catalog = cat;
        Ok(())
    }

    fn persist_catalog(&mut self) -> Result<()> {
        let bytes = encode_catalog(&self.catalog);
        let new_root = write_catalog_blob(&mut self.pager, &bytes)?;
        let old_root = self.pager.header().catalog_root;
        self.pager.header_mut().catalog_root = new_root.0;
        self.pager.write_header()?;
        self.pager.flush()?;
        if old_root != 0 {
            free_catalog_chain(&mut self.pager, PageId(old_root))?;
        }
        Ok(())
    }

    fn recover(&mut self) -> Result<()> {
        let records = wal::read_all(&self.wal_path)?;
        if records.is_empty() {
            return Ok(());
        }
        let committed = wal::committed_records(records);
        for rec in committed {
            self.apply_record(&rec)?;
        }
        self.pager.flush()?;
        self.persist_catalog()?;
        self.wal.truncate()?;
        Ok(())
    }

    fn apply_record(&mut self, rec: &WalRecord) -> Result<()> {
        match rec {
            WalRecord::Begin { .. } | WalRecord::Commit { .. } => Ok(()),
            WalRecord::Insert { table, values, .. } => self.apply_insert(table, values).map(|_| ()),
            WalRecord::Delete { table, values, .. } => {
                let rids = self.find_rows_matching_image(table, values)?;
                for rid in rids.into_iter().take(1) {
                    self.apply_delete(table, rid)?;
                }
                Ok(())
            }
            WalRecord::Update {
                table, old, new, ..
            } => {
                let rids = self.find_rows_matching_image(table, old)?;
                if let Some(rid) = rids.into_iter().next() {
                    self.apply_update(table, rid, new)?;
                }
                Ok(())
            }
            WalRecord::CreateTable { name, columns, .. } => self.apply_create_table(name, columns),
            WalRecord::DropTable { name, .. } => self.apply_drop_table(name),
            WalRecord::CreateIndex {
                name,
                table,
                column,
                ..
            } => self.apply_create_index(name, table, column),
            WalRecord::DropIndex { name, .. } => self.apply_drop_index(name),
        }
    }

    fn find_rows_matching_image(&mut self, table: &str, image: &[Value]) -> Result<Vec<RowId>> {
        let root = self.catalog.get_table(table)?.heap_root;
        let rows = Heap::scan_collect(&mut self.pager, root)?;
        let mut out = Vec::new();
        for (rid, row) in rows {
            if &row[..] == image {
                out.push(rid);
            }
        }
        Ok(out)
    }

    // ---- DDL primitives ----
    fn apply_create_table(&mut self, name: &str, columns: &[ColumnDef]) -> Result<()> {
        if self.catalog.get_table(name).is_ok() {
            return Ok(());
        }
        let heap_root = Heap::create(&mut self.pager)?;
        let table = Table {
            name: name.to_string(),
            columns: columns.to_vec(),
            heap_root,
            indexes: vec![],
        };
        self.catalog.create_table(table)?;

        // Auto-build a unique B-tree index for any PRIMARY KEY column.
        let pk_col = columns.iter().find(|c| c.primary_key).cloned();
        if let Some(pk) = pk_col {
            let idx_name = format!("__pk_{}", name.to_ascii_lowercase());
            let root = BTree::create(&mut self.pager)?;
            self.catalog.add_index(Index {
                name: idx_name.clone(),
                table: name.to_string(),
                column: pk.name.clone(),
                unique: true,
                root,
            })?;
        }
        // Also auto-build unique indexes for UNIQUE columns (non-PK).
        let uniques: Vec<ColumnDef> = columns
            .iter()
            .filter(|c| c.unique && !c.primary_key)
            .cloned()
            .collect();
        for u in uniques {
            let idx_name = format!(
                "__uniq_{}_{}",
                name.to_ascii_lowercase(),
                u.name.to_ascii_lowercase()
            );
            let root = BTree::create(&mut self.pager)?;
            self.catalog.add_index(Index {
                name: idx_name,
                table: name.to_string(),
                column: u.name,
                unique: true,
                root,
            })?;
        }
        Ok(())
    }

    fn apply_drop_table(&mut self, name: &str) -> Result<()> {
        let table = match self.catalog.get_table(name) {
            Ok(t) => t.clone(),
            Err(_) => return Ok(()),
        };
        // Drop heap pages.
        Heap::drop_heap(&mut self.pager, table.heap_root)?;
        // Drop indexes.
        let idx_names: Vec<String> = table.indexes.clone();
        for n in idx_names {
            if let Ok(idx) = self.catalog.get_index(&n) {
                let root = idx.root;
                BTree::drop_tree(&mut self.pager, root)?;
            }
        }
        self.catalog.drop_table(name)?;
        Ok(())
    }

    fn apply_create_index(&mut self, name: &str, table: &str, column: &str) -> Result<()> {
        if self.catalog.get_index(name).is_ok() {
            return Ok(());
        }
        let table_owned = self.catalog.get_table(table)?.clone();
        let col = table_owned
            .column(column)
            .ok_or_else(|| MiniDbError::catalog(format!("no such column: {}.{}", table, column)))?;
        let unique = col.unique || col.primary_key;
        let root = BTree::create(&mut self.pager)?;
        self.catalog.add_index(Index {
            name: name.to_string(),
            table: table.to_string(),
            column: column.to_string(),
            unique,
            root,
        })?;

        // Backfill: read all rows from the heap and insert into the new index.
        let col_idx = table_owned.column_index(column).unwrap();
        let rows = Heap::scan_collect(&mut self.pager, table_owned.heap_root)?;
        let idx_root = self.catalog.get_index(name)?.root;
        for (rid, values) in rows {
            BTree::insert(
                &mut self.pager,
                idx_root,
                IndexEntry::new(values[col_idx].clone(), rid),
            )?;
        }
        Ok(())
    }

    fn apply_drop_index(&mut self, name: &str) -> Result<()> {
        let idx = match self.catalog.get_index(name) {
            Ok(i) => i.clone(),
            Err(_) => return Ok(()),
        };
        BTree::drop_tree(&mut self.pager, idx.root)?;
        self.catalog.drop_index(name)?;
        Ok(())
    }

    // ---- DML primitives applied to disk ----
    fn apply_insert(&mut self, table: &str, values: &[Value]) -> Result<RowId> {
        let table_def = self.catalog.get_table(table)?.clone();
        let rid = Heap::insert(&mut self.pager, table_def.heap_root, values)?;
        // Update indexes.
        for idx_name in &table_def.indexes {
            let idx = self.catalog.get_index(idx_name)?.clone();
            let col_idx = table_def.column_index(&idx.column).unwrap();
            BTree::insert(
                &mut self.pager,
                idx.root,
                IndexEntry::new(values[col_idx].clone(), rid),
            )?;
        }
        Ok(rid)
    }

    fn apply_delete(&mut self, table: &str, rid: RowId) -> Result<()> {
        let table_def = self.catalog.get_table(table)?.clone();
        let row = Heap::read(&mut self.pager, rid)?;
        if let Some(values) = row {
            for idx_name in &table_def.indexes {
                let idx = self.catalog.get_index(idx_name)?.clone();
                let col_idx = table_def.column_index(&idx.column).unwrap();
                BTree::delete(
                    &mut self.pager,
                    idx.root,
                    &IndexEntry::new(values[col_idx].clone(), rid),
                )?;
            }
        }
        Heap::delete(&mut self.pager, rid)?;
        Ok(())
    }

    fn apply_update(&mut self, table: &str, rid: RowId, new_values: &[Value]) -> Result<()> {
        let table_def = self.catalog.get_table(table)?.clone();
        let old = Heap::read(&mut self.pager, rid)?;
        let new_rid = Heap::update(&mut self.pager, table_def.heap_root, rid, new_values)?;
        if let Some(old_vals) = old {
            for idx_name in &table_def.indexes {
                let idx = self.catalog.get_index(idx_name)?.clone();
                let col_idx = table_def.column_index(&idx.column).unwrap();
                if old_vals[col_idx] != new_values[col_idx] || rid != new_rid {
                    BTree::delete(
                        &mut self.pager,
                        idx.root,
                        &IndexEntry::new(old_vals[col_idx].clone(), rid),
                    )?;
                    BTree::insert(
                        &mut self.pager,
                        idx.root,
                        IndexEntry::new(new_values[col_idx].clone(), new_rid),
                    )?;
                }
            }
        }
        Ok(())
    }

    // ---- Public transaction API ----
    pub fn begin(&mut self) -> Result<()> {
        if self.txn.in_transaction() {
            return Err(MiniDbError::txn("transaction already open"));
        }
        self.txn.begin(&mut self.wal)?;
        self.buffer = TxnBuffer::new();
        Ok(())
    }

    pub fn commit(&mut self) -> Result<()> {
        if !self.txn.in_transaction() {
            return Err(MiniDbError::txn("no transaction to commit"));
        }
        let txn_id = self.txn.current_txn_id().unwrap();

        if self.buffer.is_empty() {
            // Empty txn: write a commit, fsync, truncate.
            self.wal.append(&WalRecord::Commit { txn_id })?;
            self.wal.fsync()?;
            self.wal.truncate()?;
            self.txn.end();
            return Ok(());
        }

        // 1. Append buffered records (already includes the BEGIN record).
        for rec in &self.buffer.wal_records {
            self.wal.append(rec)?;
        }
        // 2. Append commit, fsync.
        self.wal.append(&WalRecord::Commit { txn_id })?;
        self.wal.fsync()?;

        // 3. Apply to data file.
        let buf = std::mem::take(&mut self.buffer);
        self.apply_buffer(buf)?;

        // 4. Fsync data, persist catalog.
        self.persist_catalog()?;
        self.pager.flush()?;

        // 5. Truncate WAL.
        self.wal.truncate()?;
        self.txn.end();
        Ok(())
    }

    pub fn rollback(&mut self) -> Result<()> {
        if !self.txn.in_transaction() {
            return Err(MiniDbError::txn("no transaction to rollback"));
        }
        // Discard buffer. Truncate WAL (the BEGIN record is already in there).
        self.buffer = TxnBuffer::new();
        self.wal.truncate()?;
        self.txn.end();
        Ok(())
    }

    fn apply_buffer(&mut self, buf: TxnBuffer) -> Result<()> {
        // DDL first (so subsequent DML hits the correct catalog state).
        for op in buf.ddl {
            match op {
                DdlOp::CreateTable { name, columns } => self.apply_create_table(&name, &columns)?,
                DdlOp::DropTable { name } => self.apply_drop_table(&name)?,
                DdlOp::CreateIndex {
                    name,
                    table,
                    column,
                } => self.apply_create_index(&name, &table, &column)?,
                DdlOp::DropIndex { name } => self.apply_drop_index(&name)?,
            }
        }
        // Apply deletes against persistent rids first, then updates, then inserts.
        for (table, rids) in buf.deletes {
            for rid in rids {
                self.apply_delete(&table, rid)?;
            }
        }
        for (table, updates) in buf.updates {
            for (rid, new_row) in updates {
                self.apply_update(&table, rid, &new_row)?;
            }
        }
        for (table, rows) in buf.inserts {
            for row in rows {
                self.apply_insert(&table, &row)?;
            }
        }
        Ok(())
    }

    // ---- High-level DML/DDL entry points (used by executor) ----

    /// Auto-commit a single DDL operation immediately.
    pub fn ddl_create_table(&mut self, name: &str, columns: Vec<ColumnDef>) -> Result<()> {
        self.implicit_commit_if_open()?;
        let txn_id = self.txn.allocate_id();
        self.wal.append(&WalRecord::Begin { txn_id })?;
        self.wal.append(&WalRecord::CreateTable {
            txn_id,
            name: name.to_string(),
            columns: columns.clone(),
        })?;
        self.wal.append(&WalRecord::Commit { txn_id })?;
        self.wal.fsync()?;
        self.apply_create_table(name, &columns)?;
        self.persist_catalog()?;
        self.pager.flush()?;
        self.wal.truncate()?;
        Ok(())
    }

    pub fn ddl_drop_table(&mut self, name: &str) -> Result<()> {
        self.implicit_commit_if_open()?;
        let txn_id = self.txn.allocate_id();
        self.wal.append(&WalRecord::Begin { txn_id })?;
        self.wal.append(&WalRecord::DropTable {
            txn_id,
            name: name.to_string(),
        })?;
        self.wal.append(&WalRecord::Commit { txn_id })?;
        self.wal.fsync()?;
        self.apply_drop_table(name)?;
        self.persist_catalog()?;
        self.pager.flush()?;
        self.wal.truncate()?;
        Ok(())
    }

    pub fn ddl_create_index(&mut self, name: &str, table: &str, column: &str) -> Result<()> {
        self.implicit_commit_if_open()?;
        let txn_id = self.txn.allocate_id();
        self.wal.append(&WalRecord::Begin { txn_id })?;
        self.wal.append(&WalRecord::CreateIndex {
            txn_id,
            name: name.to_string(),
            table: table.to_string(),
            column: column.to_string(),
        })?;
        self.wal.append(&WalRecord::Commit { txn_id })?;
        self.wal.fsync()?;
        self.apply_create_index(name, table, column)?;
        self.persist_catalog()?;
        self.pager.flush()?;
        self.wal.truncate()?;
        Ok(())
    }

    pub fn ddl_drop_index(&mut self, name: &str) -> Result<()> {
        self.implicit_commit_if_open()?;
        let txn_id = self.txn.allocate_id();
        self.wal.append(&WalRecord::Begin { txn_id })?;
        self.wal.append(&WalRecord::DropIndex {
            txn_id,
            name: name.to_string(),
        })?;
        self.wal.append(&WalRecord::Commit { txn_id })?;
        self.wal.fsync()?;
        self.apply_drop_index(name)?;
        self.persist_catalog()?;
        self.pager.flush()?;
        self.wal.truncate()?;
        Ok(())
    }

    fn implicit_commit_if_open(&mut self) -> Result<()> {
        if self.txn.in_transaction() {
            self.commit()?;
        }
        Ok(())
    }

    /// Insert a row into `table`. Returns the persistent row id if applied
    /// immediately (auto-commit), otherwise the buffered insert is returned
    /// without a rowid.
    pub fn dml_insert(&mut self, table: &str, values: Vec<Value>) -> Result<()> {
        if self.txn.in_transaction() {
            let txn_id = self.txn.current_txn_id().unwrap();
            self.buffer.record_insert(
                table,
                values.clone(),
                WalRecord::Insert {
                    txn_id,
                    table: table.to_string(),
                    values,
                },
            );
            return Ok(());
        }
        // Auto-commit.
        let txn_id = self.txn.allocate_id();
        self.wal.append(&WalRecord::Begin { txn_id })?;
        self.wal.append(&WalRecord::Insert {
            txn_id,
            table: table.to_string(),
            values: values.clone(),
        })?;
        self.wal.append(&WalRecord::Commit { txn_id })?;
        self.wal.fsync()?;
        self.apply_insert(table, &values)?;
        self.pager.flush()?;
        self.wal.truncate()?;
        Ok(())
    }

    pub fn dml_delete_persistent(&mut self, table: &str, rid: RowId) -> Result<()> {
        let row = Heap::read(&mut self.pager, rid)?
            .ok_or_else(|| MiniDbError::runtime("row vanished during delete"))?;
        if self.txn.in_transaction() {
            let txn_id = self.txn.current_txn_id().unwrap();
            self.buffer.record_delete(
                table,
                rid,
                WalRecord::Delete {
                    txn_id,
                    table: table.to_string(),
                    values: row,
                },
            );
            return Ok(());
        }
        let txn_id = self.txn.allocate_id();
        self.wal.append(&WalRecord::Begin { txn_id })?;
        self.wal.append(&WalRecord::Delete {
            txn_id,
            table: table.to_string(),
            values: row,
        })?;
        self.wal.append(&WalRecord::Commit { txn_id })?;
        self.wal.fsync()?;
        self.apply_delete(table, rid)?;
        self.pager.flush()?;
        self.wal.truncate()?;
        Ok(())
    }

    pub fn dml_delete_pending(&mut self, table: &str, idx: usize) -> Result<()> {
        let key = table.to_ascii_lowercase();
        if let Some(v) = self.buffer.inserts.get_mut(&key) {
            if idx < v.len() {
                // Remove and shift. Also remove its WAL record (best-effort by
                // matching values).
                v.remove(idx);
            }
        }
        Ok(())
    }

    pub fn dml_update_persistent(
        &mut self,
        table: &str,
        rid: RowId,
        new_row: Vec<Value>,
    ) -> Result<()> {
        let old = Heap::read(&mut self.pager, rid)?
            .ok_or_else(|| MiniDbError::runtime("row vanished during update"))?;
        if self.txn.in_transaction() {
            let txn_id = self.txn.current_txn_id().unwrap();
            self.buffer.record_update(
                table,
                rid,
                new_row.clone(),
                WalRecord::Update {
                    txn_id,
                    table: table.to_string(),
                    old,
                    new: new_row,
                },
            );
            return Ok(());
        }
        let txn_id = self.txn.allocate_id();
        self.wal.append(&WalRecord::Begin { txn_id })?;
        self.wal.append(&WalRecord::Update {
            txn_id,
            table: table.to_string(),
            old,
            new: new_row.clone(),
        })?;
        self.wal.append(&WalRecord::Commit { txn_id })?;
        self.wal.fsync()?;
        self.apply_update(table, rid, &new_row)?;
        self.pager.flush()?;
        self.wal.truncate()?;
        Ok(())
    }

    pub fn dml_update_pending(
        &mut self,
        table: &str,
        idx: usize,
        new_row: Vec<Value>,
    ) -> Result<()> {
        let key = table.to_ascii_lowercase();
        if let Some(v) = self.buffer.inserts.get_mut(&key) {
            if idx < v.len() {
                v[idx] = new_row;
            }
        }
        Ok(())
    }

    /// Yield every visible row of `table`, applying buffered updates/deletes
    /// and including pending inserts.
    pub fn scan_table(&mut self, table: &str) -> Result<Vec<VisibleRow>> {
        let table_def = self.catalog.get_table(table)?.clone();
        let mut rows = Heap::scan_collect(&mut self.pager, table_def.heap_root)?;
        let key = table.to_ascii_lowercase();
        let deletes = self.buffer.deletes.get(&key).cloned().unwrap_or_default();
        let updates = self.buffer.updates.get(&key).cloned().unwrap_or_default();
        let inserts = self.buffer.inserts.get(&key).cloned().unwrap_or_default();

        // Filter deleted, override updated.
        rows.retain(|(rid, _)| !deletes.contains(rid));
        for (rid, row) in rows.iter_mut() {
            if let Some(new) = updates.get(rid) {
                *row = new.clone();
            }
        }

        let mut out: Vec<VisibleRow> = rows
            .into_iter()
            .map(|(rid, values)| VisibleRow {
                rref: RowRef::Persistent(rid),
                values,
            })
            .collect();
        for (i, vals) in inserts.into_iter().enumerate() {
            out.push(VisibleRow {
                rref: RowRef::Pending(i),
                values: vals,
            });
        }
        Ok(out)
    }

    /// Index probe (equality). Honors the buffered txn state.
    pub fn index_lookup_eq(
        &mut self,
        table: &str,
        column: &str,
        key: &Value,
    ) -> Result<Vec<VisibleRow>> {
        // For correctness with buffered DML, fall back to the table scan.
        let rows = self.scan_table(table)?;
        let table_def = self.catalog.get_table(table)?.clone();
        let col_idx = table_def
            .column_index(column)
            .ok_or_else(|| MiniDbError::ty(format!("no such column: {}.{}", table, column)))?;
        // Use the index on disk only when no buffered changes for this table.
        let key_lc = table.to_ascii_lowercase();
        let buffered = self.buffer.inserts.contains_key(&key_lc)
            || self.buffer.deletes.contains_key(&key_lc)
            || self.buffer.updates.contains_key(&key_lc);
        if !buffered {
            if let Some(idx) = self.catalog.find_index_for(table, column) {
                let root = idx.root;
                let rids = BTree::lookup_eq(&mut self.pager, root, key)?;
                let mut out = Vec::new();
                for rid in rids {
                    if let Some(values) = Heap::read(&mut self.pager, rid)? {
                        out.push(VisibleRow {
                            rref: RowRef::Persistent(rid),
                            values,
                        });
                    }
                }
                return Ok(out);
            }
        }
        // Fallback: filter the full scan.
        Ok(rows
            .into_iter()
            .filter(|r| {
                crate::types::Value::cmp_sql(&r.values[col_idx], key)
                    .map(|o| o == std::cmp::Ordering::Equal)
                    .unwrap_or(false)
            })
            .collect())
    }
}

fn wal_path_for(db_path: &Path) -> PathBuf {
    let mut p = db_path.as_os_str().to_owned();
    p.push(".wal");
    PathBuf::from(p)
}

// ---- Catalog persistence (chained pages) ----
fn encode_catalog(cat: &Catalog) -> Vec<u8> {
    use crate::storage::codec;
    let mut out = Vec::new();
    let tables: Vec<_> = cat.tables().collect();
    out.extend_from_slice(&(tables.len() as u32).to_le_bytes());
    for t in tables {
        write_str(&mut out, &t.name);
        out.extend_from_slice(&t.heap_root.0.to_le_bytes());
        out.extend_from_slice(&(t.columns.len() as u32).to_le_bytes());
        for c in &t.columns {
            write_str(&mut out, &c.name);
            out.push(match c.data_type {
                DataType::Int => 0,
                DataType::Real => 1,
                DataType::Text => 2,
                DataType::Bool => 3,
            });
            let mut flags = 0u8;
            if c.primary_key {
                flags |= 0b0001;
            }
            if c.not_null {
                flags |= 0b0010;
            }
            if c.unique {
                flags |= 0b0100;
            }
            if c.default.is_some() {
                flags |= 0b1000;
            }
            out.push(flags);
            if let Some(d) = &c.default {
                let _ = codec::encode_value(d, &mut out);
            }
        }
    }
    let indexes: Vec<_> = cat.indexes().collect();
    out.extend_from_slice(&(indexes.len() as u32).to_le_bytes());
    for i in indexes {
        write_str(&mut out, &i.name);
        write_str(&mut out, &i.table);
        write_str(&mut out, &i.column);
        out.push(if i.unique { 1 } else { 0 });
        out.extend_from_slice(&i.root.0.to_le_bytes());
    }
    out
}

fn decode_catalog(buf: &[u8]) -> Result<Catalog> {
    use crate::storage::codec;
    let mut cat = Catalog::new();
    let mut pos = 0usize;
    let n = read_u32(buf, &mut pos)?;
    let mut table_indexes_to_attach: HashMap<String, Vec<String>> = HashMap::new();
    for _ in 0..n {
        let name = read_str_buf(buf, &mut pos)?;
        let heap_root = PageId(read_u64_buf(buf, &mut pos)?);
        let nc = read_u32(buf, &mut pos)?;
        let mut cols = Vec::with_capacity(nc as usize);
        for _ in 0..nc {
            let cname = read_str_buf(buf, &mut pos)?;
            let dt_byte = read_u8(buf, &mut pos)?;
            let dt = match dt_byte {
                0 => DataType::Int,
                1 => DataType::Real,
                2 => DataType::Text,
                3 => DataType::Bool,
                _ => return Err(MiniDbError::storage("bad dt in catalog")),
            };
            let flags = read_u8(buf, &mut pos)?;
            let mut col = ColumnDef::new(cname, dt);
            col.primary_key = flags & 0b0001 != 0;
            col.not_null = flags & 0b0010 != 0;
            col.unique = flags & 0b0100 != 0;
            if flags & 0b1000 != 0 {
                let (v, used) = codec::decode_value(&buf[pos..])?;
                pos += used;
                col.default = Some(v);
            }
            cols.push(col);
        }
        cat.create_table(Table {
            name,
            columns: cols,
            heap_root,
            indexes: vec![],
        })?;
    }
    let ni = read_u32(buf, &mut pos)?;
    for _ in 0..ni {
        let name = read_str_buf(buf, &mut pos)?;
        let table = read_str_buf(buf, &mut pos)?;
        let column = read_str_buf(buf, &mut pos)?;
        let unique = read_u8(buf, &mut pos)? != 0;
        let root = PageId(read_u64_buf(buf, &mut pos)?);
        table_indexes_to_attach
            .entry(table.to_ascii_lowercase())
            .or_default()
            .push(name.clone());
        cat.add_index(Index {
            name,
            table,
            column,
            unique,
            root,
        })?;
    }
    let _ = HashSet::<u8>::new();
    Ok(cat)
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

fn read_u8(buf: &[u8], pos: &mut usize) -> Result<u8> {
    let v = *buf
        .get(*pos)
        .ok_or_else(|| MiniDbError::storage("truncated u8"))?;
    *pos += 1;
    Ok(v)
}

fn read_u32(buf: &[u8], pos: &mut usize) -> Result<u32> {
    if buf.len() < *pos + 4 {
        return Err(MiniDbError::storage("truncated u32"));
    }
    let mut a = [0u8; 4];
    a.copy_from_slice(&buf[*pos..*pos + 4]);
    *pos += 4;
    Ok(u32::from_le_bytes(a))
}

fn read_u64_buf(buf: &[u8], pos: &mut usize) -> Result<u64> {
    if buf.len() < *pos + 8 {
        return Err(MiniDbError::storage("truncated u64"));
    }
    let mut a = [0u8; 8];
    a.copy_from_slice(&buf[*pos..*pos + 8]);
    *pos += 8;
    Ok(u64::from_le_bytes(a))
}

fn read_str_buf(buf: &[u8], pos: &mut usize) -> Result<String> {
    let n = read_u32(buf, pos)? as usize;
    if buf.len() < *pos + n {
        return Err(MiniDbError::storage("truncated string"));
    }
    let s = std::str::from_utf8(&buf[*pos..*pos + n])
        .map_err(|_| MiniDbError::storage("bad utf8 in catalog"))?
        .to_string();
    *pos += n;
    Ok(s)
}

const CAT_PAGE_HEADER: usize = 12; // u64 next, u32 chunk_len

fn write_catalog_blob(pager: &mut Pager, blob: &[u8]) -> Result<PageId> {
    let chunk_max = PAGE_SIZE - CAT_PAGE_HEADER;
    let mut pages = Vec::new();
    let mut idx = 0;
    while idx < blob.len().max(1) {
        pages.push(pager.allocate_page()?);
        if blob.is_empty() {
            break;
        }
        idx += chunk_max;
    }
    if pages.is_empty() {
        pages.push(pager.allocate_page()?);
    }

    let total_chunks = pages.len();
    for i in 0..total_chunks {
        let mut buf = vec![0u8; PAGE_SIZE];
        let next = if i + 1 < total_chunks {
            pages[i + 1].0
        } else {
            0
        };
        buf[0..8].copy_from_slice(&next.to_le_bytes());
        let start = i * chunk_max;
        let end = std::cmp::min(blob.len(), start + chunk_max);
        let chunk = if start >= blob.len() {
            &[][..]
        } else {
            &blob[start..end]
        };
        let cl = chunk.len() as u32;
        buf[8..12].copy_from_slice(&cl.to_le_bytes());
        buf[12..12 + chunk.len()].copy_from_slice(chunk);
        pager.write_page(pages[i], &buf)?;
    }
    Ok(pages[0])
}

fn read_catalog_blob(pager: &mut Pager, root: PageId) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur = root;
    loop {
        let buf = pager.read_page(cur)?;
        let mut nb = [0u8; 8];
        nb.copy_from_slice(&buf[0..8]);
        let next = u64::from_le_bytes(nb);
        let mut cl = [0u8; 4];
        cl.copy_from_slice(&buf[8..12]);
        let chunk_len = u32::from_le_bytes(cl) as usize;
        out.extend_from_slice(&buf[12..12 + chunk_len]);
        if next == 0 {
            break;
        }
        cur = PageId(next);
    }
    Ok(out)
}

fn free_catalog_chain(pager: &mut Pager, root: PageId) -> Result<()> {
    let mut chain = Vec::new();
    let mut cur = root;
    loop {
        let buf = pager.read_page(cur)?;
        let mut nb = [0u8; 8];
        nb.copy_from_slice(&buf[0..8]);
        let next = u64::from_le_bytes(nb);
        chain.push(cur);
        if next == 0 {
            break;
        }
        cur = PageId(next);
    }
    for p in chain {
        pager.free_page(p)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static C: AtomicU64 = AtomicU64::new(0);

    fn tmp() -> PathBuf {
        let n = C.fetch_add(1, Ordering::SeqCst);
        let mut p = temp_dir();
        p.push(format!("minidb_engine_{}_{}.db", std::process::id(), n));
        let _ = std::fs::remove_file(&p);
        let mut wp = p.clone().into_os_string();
        wp.push(".wal");
        let _ = std::fs::remove_file(PathBuf::from(wp));
        p
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
        let mut wp = path.as_os_str().to_owned();
        wp.push(".wal");
        let _ = std::fs::remove_file(PathBuf::from(wp));
    }

    fn col(n: &str, t: DataType) -> ColumnDef {
        ColumnDef::new(n, t)
    }

    #[test]
    fn create_insert_scan_persists() {
        let path = tmp();
        {
            let mut e = Engine::open(&path).unwrap();
            e.ddl_create_table(
                "t",
                vec![col("id", DataType::Int), col("name", DataType::Text)],
            )
            .unwrap();
            e.dml_insert("t", vec![Value::Int(1), Value::Text("a".into())])
                .unwrap();
            e.dml_insert("t", vec![Value::Int(2), Value::Text("b".into())])
                .unwrap();
        }
        {
            let mut e = Engine::open(&path).unwrap();
            let rows = e.scan_table("t").unwrap();
            assert_eq!(rows.len(), 2);
        }
        cleanup(&path);
    }

    #[test]
    fn rollback_discards() {
        let path = tmp();
        {
            let mut e = Engine::open(&path).unwrap();
            e.ddl_create_table("t", vec![col("id", DataType::Int)])
                .unwrap();
            e.begin().unwrap();
            e.dml_insert("t", vec![Value::Int(99)]).unwrap();
            assert_eq!(e.scan_table("t").unwrap().len(), 1);
            e.rollback().unwrap();
            assert_eq!(e.scan_table("t").unwrap().len(), 0);
        }
        cleanup(&path);
    }

    #[test]
    fn commit_makes_visible_after_reopen() {
        let path = tmp();
        {
            let mut e = Engine::open(&path).unwrap();
            e.ddl_create_table("t", vec![col("id", DataType::Int)])
                .unwrap();
            e.begin().unwrap();
            e.dml_insert("t", vec![Value::Int(7)]).unwrap();
            e.dml_insert("t", vec![Value::Int(8)]).unwrap();
            e.commit().unwrap();
        }
        {
            let mut e = Engine::open(&path).unwrap();
            let rows = e.scan_table("t").unwrap();
            assert_eq!(rows.len(), 2);
        }
        cleanup(&path);
    }
}

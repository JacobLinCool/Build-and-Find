use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, IndexSchema, TableSchema};
use crate::error::{MiniDbError, Result};
use crate::storage::btree::BTreeIndex;
use crate::storage::heap::{HeapTable, RowId};
use crate::storage::wal::{Wal, WalKind, WalRecord};
use crate::storage::PageManager;
use crate::types::Row;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableData {
    pub schema: TableSchema,
    pub heap: HeapTable,
    pub indexes: BTreeMap<String, BTreeIndex>,
}

impl TableData {
    pub fn new(schema: TableSchema) -> Self {
        Self {
            schema,
            heap: HeapTable::default(),
            indexes: BTreeMap::new(),
        }
    }

    pub fn rebuild_indexes(&mut self) -> Result<()> {
        let mut rebuilt = BTreeMap::new();
        for (name, _index) in &self.indexes {
            let column = self
                .schema
                .columns
                .iter()
                .position(|c| c.name == *name)
                .ok_or_else(|| MiniDbError::Corrupt(format!("missing indexed column {name}")))?;
            let mut new_index = BTreeIndex::default();
            for (row_id, row) in self.heap.iter() {
                if let Some(value) = row.get(column) {
                    new_index.insert(value.clone(), row_id);
                }
            }
            rebuilt.insert(name.clone(), new_index);
        }
        self.indexes = rebuilt;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseState {
    pub catalog: Catalog,
    pub tables: BTreeMap<String, TableData>,
    pub last_committed_txid: u64,
}

impl Default for DatabaseState {
    fn default() -> Self {
        Self {
            catalog: Catalog::default(),
            tables: BTreeMap::new(),
            last_committed_txid: 0,
        }
    }
}

impl DatabaseState {
    pub fn table(&self, name: &str) -> Result<&TableData> {
        self.tables
            .get(name)
            .ok_or_else(|| MiniDbError::Runtime(format!("unknown table {name}")))
    }

    pub fn table_mut(&mut self, name: &str) -> Result<&mut TableData> {
        self.tables
            .get_mut(name)
            .ok_or_else(|| MiniDbError::Runtime(format!("unknown table {name}")))
    }

    pub fn create_table(&mut self, schema: TableSchema) -> Result<()> {
        if self.catalog.tables.contains_key(&schema.name) {
            return Err(MiniDbError::Runtime(format!(
                "table {} already exists",
                schema.name
            )));
        }
        self.catalog.create_table(schema.clone());
        self.tables
            .insert(schema.name.clone(), TableData::new(schema));
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        if self.catalog.tables.remove(name).is_none() {
            return Err(MiniDbError::Runtime(format!("unknown table {name}")));
        }
        self.tables.remove(name);
        self.catalog.indexes.retain(|_, idx| idx.table != name);
        Ok(())
    }

    pub fn create_index(&mut self, index: IndexSchema) -> Result<()> {
        if self.catalog.indexes.contains_key(&index.name) {
            return Err(MiniDbError::Runtime(format!(
                "index {} already exists",
                index.name
            )));
        }
        let table = self.table_mut(&index.table)?;
        let col_idx = table
            .schema
            .columns
            .iter()
            .position(|c| c.name == index.column)
            .ok_or_else(|| MiniDbError::Runtime(format!("unknown column {}", index.column)))?;
        let mut btree = BTreeIndex::default();
        for (row_id, row) in table.heap.iter() {
            if let Some(value) = row.get(col_idx) {
                btree.insert(value.clone(), row_id);
            }
        }
        table.indexes.insert(index.column.clone(), btree);
        self.catalog.create_index(index);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<()> {
        let index = self
            .catalog
            .indexes
            .remove(name)
            .ok_or_else(|| MiniDbError::Runtime(format!("unknown index {name}")))?;
        let table = self.table_mut(&index.table)?;
        table.indexes.remove(&index.column);
        Ok(())
    }

    pub fn insert_row(&mut self, table_name: &str, row: Row) -> Result<RowId> {
        let table = self.table_mut(table_name)?;
        let row_id = table.heap.insert(row);
        table.rebuild_indexes()?;
        Ok(row_id)
    }

    pub fn update_rows(&mut self, table_name: &str, rows: &[(RowId, Row)]) -> Result<usize> {
        let table = self.table_mut(table_name)?;
        for (row_id, row) in rows {
            table.heap.rows.insert(*row_id, row.clone());
        }
        table.rebuild_indexes()?;
        Ok(rows.len())
    }

    pub fn delete_rows(&mut self, table_name: &str, row_ids: &[RowId]) -> Result<usize> {
        let table = self.table_mut(table_name)?;
        for row_id in row_ids {
            table.heap.delete(*row_id);
        }
        table.rebuild_indexes()?;
        Ok(row_ids.len())
    }
}

#[derive(Debug, Clone)]
struct WorkingTxn {
    txid: u64,
    state: DatabaseState,
    sql_log: Vec<String>,
}

#[derive(Debug)]
pub struct TransactionManager {
    pub page_manager: PageManager,
    pub wal: Wal,
    committed: DatabaseState,
    active: Option<WorkingTxn>,
    next_txid: u64,
}

impl TransactionManager {
    pub fn open(base_path: impl Into<PathBuf>) -> Result<Self> {
        let base_path = base_path.into();
        let page_manager = PageManager::new(&base_path);
        let wal = Wal::new(wal_path(&base_path));
        let mut committed = if let Some((snapshot_lsn, payload)) = page_manager.load_snapshot()? {
            let mut state: DatabaseState = bincode::deserialize(&payload)
                .map_err(|err| MiniDbError::Corrupt(err.to_string()))?;
            state.last_committed_txid = snapshot_lsn;
            state
        } else {
            DatabaseState::default()
        };
        let records = wal.read_all()?;
        let mut max_txid = committed.last_committed_txid;
        let mut tx_buffer: BTreeMap<u64, Vec<String>> = BTreeMap::new();
        let mut seen_commit: BTreeMap<u64, bool> = BTreeMap::new();
        for record in records {
            max_txid = max_txid.max(record.txid);
            match record.kind {
                WalKind::Begin => {
                    tx_buffer.entry(record.txid).or_default();
                }
                WalKind::Statement { sql } => {
                    tx_buffer.entry(record.txid).or_default().push(sql);
                }
                WalKind::Commit => {
                    seen_commit.insert(record.txid, true);
                }
                WalKind::Rollback => {
                    tx_buffer.remove(&record.txid);
                    seen_commit.remove(&record.txid);
                }
            }
        }
        for (txid, statements) in tx_buffer {
            if txid <= committed.last_committed_txid {
                continue;
            }
            if !seen_commit.get(&txid).copied().unwrap_or(false) {
                continue;
            }
            for sql in statements {
                // Recovery replays committed statement text through the normal engine.
                let stmt = crate::parser::parse(&sql)?;
                for statement in stmt {
                    crate::plan::apply_statement_without_wal(&mut committed, &statement)?;
                }
            }
            committed.last_committed_txid = txid;
        }
        let next_txid = max_txid.max(committed.last_committed_txid) + 1;
        Ok(Self {
            page_manager,
            wal,
            committed,
            active: None,
            next_txid,
        })
    }

    pub fn state(&self) -> &DatabaseState {
        self.active
            .as_ref()
            .map(|t| &t.state)
            .unwrap_or(&self.committed)
    }

    pub fn state_mut(&mut self) -> &mut DatabaseState {
        if let Some(active) = self.active.as_mut() {
            &mut active.state
        } else {
            &mut self.committed
        }
    }

    pub fn begin(&mut self) -> Result<()> {
        if self.active.is_some() {
            return Err(MiniDbError::Runtime("transaction already open".into()));
        }
        let txid = self.next_txid;
        self.next_txid += 1;
        self.wal.append(&WalRecord {
            txid,
            kind: WalKind::Begin,
        })?;
        self.active = Some(WorkingTxn {
            txid,
            state: self.committed.clone(),
            sql_log: Vec::new(),
        });
        Ok(())
    }

    pub fn record_sql(&mut self, sql: &str) -> Result<()> {
        if let Some(active) = self.active.as_mut() {
            active.sql_log.push(sql.to_string());
            Ok(())
        } else {
            let txid = self.next_txid;
            self.next_txid += 1;
            self.wal.append(&WalRecord {
                txid,
                kind: WalKind::Begin,
            })?;
            self.wal.append(&WalRecord {
                txid,
                kind: WalKind::Statement {
                    sql: sql.to_string(),
                },
            })?;
            self.wal.append(&WalRecord {
                txid,
                kind: WalKind::Commit,
            })?;
            self.persist_committed(txid)?;
            Ok(())
        }
    }

    pub fn commit(&mut self) -> Result<()> {
        let active = self
            .active
            .take()
            .ok_or_else(|| MiniDbError::Runtime("no transaction open".into()))?;
        for sql in &active.sql_log {
            self.wal.append(&WalRecord {
                txid: active.txid,
                kind: WalKind::Statement { sql: sql.clone() },
            })?;
        }
        self.wal.append(&WalRecord {
            txid: active.txid,
            kind: WalKind::Commit,
        })?;
        self.committed = active.state;
        self.persist_committed(active.txid)?;
        Ok(())
    }

    pub fn rollback(&mut self) -> Result<()> {
        let active = self
            .active
            .take()
            .ok_or_else(|| MiniDbError::Runtime("no transaction open".into()))?;
        self.wal.append(&WalRecord {
            txid: active.txid,
            kind: WalKind::Rollback,
        })?;
        Ok(())
    }

    fn persist_committed(&mut self, txid: u64) -> Result<()> {
        self.committed.last_committed_txid = txid;
        let payload = bincode::serialize(&self.committed)
            .map_err(|err| MiniDbError::Storage(err.to_string()))?;
        self.page_manager.store_snapshot(txid, &payload)?;
        Ok(())
    }

    pub fn execute_in_txn<F, T>(&mut self, sql: &str, mut f: F) -> Result<T>
    where
        F: FnMut(&mut DatabaseState) -> Result<T>,
    {
        if self.active.is_some() {
            self.record_sql(sql)?;
            let snapshot = self.state().clone();
            match f(self.state_mut()) {
                Ok(result) => Ok(result),
                Err(err) => {
                    if let Some(active) = self.active.as_mut() {
                        active.sql_log.pop();
                        active.state = snapshot;
                    }
                    Err(err)
                }
            }
        } else {
            self.begin()?;
            self.record_sql(sql)?;
            let snapshot = self.state().clone();
            match f(self.state_mut()) {
                Ok(result) => {
                    self.commit()?;
                    Ok(result)
                }
                Err(err) => {
                    if let Some(active) = self.active.as_mut() {
                        active.sql_log.pop();
                        active.state = snapshot;
                    }
                    let _ = self.rollback();
                    Err(err)
                }
            }
        }
    }

    pub fn with_state_mut<F, T>(&mut self, f: F) -> Result<T>
    where
        F: FnOnce(&mut DatabaseState) -> Result<T>,
    {
        f(self.state_mut())
    }

    pub fn read_state(&self) -> &DatabaseState {
        self.state()
    }

    pub fn base_path(&self) -> &Path {
        self.page_manager.path()
    }
}

fn wal_path(base: &Path) -> PathBuf {
    let mut wal = base.as_os_str().to_os_string();
    wal.push(".wal");
    PathBuf::from(wal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn transaction_lifecycle() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("db");
        let mut tm = TransactionManager::open(&db).unwrap();
        tm.begin().unwrap();
        tm.rollback().unwrap();
    }
}

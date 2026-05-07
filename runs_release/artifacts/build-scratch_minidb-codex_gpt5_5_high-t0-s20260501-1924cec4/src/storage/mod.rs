pub mod btree;
pub mod heap;
pub mod page;
pub mod wal;

use crate::catalog::{normalize, Catalog};
use crate::error::{MiniDbError, Result};
use crate::parser::ast::ColumnDef;
use crate::storage::btree::BTreeIndex;
use crate::storage::heap::{HeapTable, StoredRow};
use crate::storage::wal::{Wal, WalOp, WalRecord};
use crate::txn::TransactionManager;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PageId(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseState {
    pub catalog: Catalog,
    pub tables: BTreeMap<String, HeapTable>,
    pub indexes: BTreeMap<String, BTreeIndex>,
    pub next_row_id: u64,
    pub applied_tx: u64,
}

impl Default for DatabaseState {
    fn default() -> Self {
        Self {
            catalog: Catalog::default(),
            tables: BTreeMap::new(),
            indexes: BTreeMap::new(),
            next_row_id: 1,
            applied_tx: 0,
        }
    }
}

pub struct Database {
    pub path: PathBuf,
    pub wal_path: PathBuf,
    pub state: DatabaseState,
    txn: TransactionManager,
    savepoint: Option<DatabaseState>,
}

impl Database {
    pub fn init(path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let state = DatabaseState::default();
        write_checkpoint(path, &state)?;
        let wal_path = wal_path_for(path);
        if wal_path.exists() {
            fs::remove_file(wal_path)?;
        }
        Ok(())
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            write_checkpoint(&path, &DatabaseState::default())?;
        }
        let wal_path = wal_path_for(&path);
        let mut state = read_checkpoint(&path)?;
        let mut max_tx = state.applied_tx;
        if wal_path.exists() {
            let records = Wal::read_all(&wal_path)?;
            let committed = committed_ops(&records);
            for (tx, ops) in committed {
                max_tx = max_tx.max(tx);
                if tx <= state.applied_tx {
                    continue;
                }
                for op in ops {
                    apply_op_to_state(&mut state, op)?;
                }
                state.applied_tx = tx;
            }
            state.rebuild_indexes()?;
            if state.applied_tx > 0 {
                write_checkpoint(&path, &state)?;
            }
        }
        let mut txn = TransactionManager::default();
        txn.seed_after(max_tx);
        Ok(Self {
            path,
            wal_path,
            state,
            txn,
            savepoint: None,
        })
    }

    pub fn begin(&mut self) -> Result<()> {
        let tx = self.txn.begin()?;
        self.savepoint = Some(self.state.clone());
        Wal::append(&self.wal_path, &WalRecord::Begin { tx })?;
        Ok(())
    }

    pub fn commit(&mut self) -> Result<()> {
        let tx = self.txn.commit()?;
        Wal::append(&self.wal_path, &WalRecord::Commit { tx })?;
        Wal::sync(&self.wal_path)?;
        self.state.applied_tx = tx;
        write_checkpoint(&self.path, &self.state)?;
        Wal::truncate(&self.wal_path)?;
        self.savepoint = None;
        Ok(())
    }

    pub fn rollback(&mut self) -> Result<()> {
        let tx = self.txn.rollback()?;
        if let Some(state) = self.savepoint.take() {
            self.state = state;
        }
        Wal::append(&self.wal_path, &WalRecord::Rollback { tx })?;
        Ok(())
    }

    pub fn in_transaction(&self) -> bool {
        self.txn.active().is_some()
    }

    pub fn current_tx(&self) -> Result<u64> {
        self.txn
            .active()
            .ok_or_else(|| MiniDbError::Sql("no active transaction".into()))
    }

    pub fn append_and_apply(&mut self, op: WalOp) -> Result<()> {
        let tx = self.current_tx()?;
        let mut next_state = self.state.clone();
        apply_op_to_state(&mut next_state, &op)?;
        next_state.rebuild_indexes()?;
        Wal::append(&self.wal_path, &WalRecord::Operation { tx, op: op.clone() })?;
        self.state = next_state;
        Ok(())
    }
}

impl DatabaseState {
    pub fn table_rows(&self, table: &str) -> Result<&Vec<StoredRow>> {
        self.tables
            .get(&normalize(table))
            .map(|t| &t.rows)
            .ok_or_else(|| MiniDbError::Sql(format!("unknown table {table}")))
    }

    pub fn table_rows_mut(&mut self, table: &str) -> Result<&mut Vec<StoredRow>> {
        self.tables
            .get_mut(&normalize(table))
            .map(|t| &mut t.rows)
            .ok_or_else(|| MiniDbError::Sql(format!("unknown table {table}")))
    }

    pub fn rebuild_indexes(&mut self) -> Result<()> {
        self.indexes.clear();
        for idx in self.state_index_specs() {
            let schema = self.catalog.table(&idx.table)?;
            let col = schema.column_index(&idx.column)?;
            let mut tree = BTreeIndex::new(idx.name.clone(), idx.table.clone(), idx.column.clone());
            if let Some(table) = self.tables.get(&normalize(&idx.table)) {
                for row in &table.rows {
                    if let Some(value) = row.values.get(col) {
                        tree.insert(value, row.id);
                    }
                }
            }
            self.indexes.insert(normalize(&idx.name), tree);
        }
        Ok(())
    }

    fn state_index_specs(&self) -> Vec<crate::catalog::IndexSchema> {
        self.catalog.indexes.values().cloned().collect()
    }
}

fn apply_op_to_state(state: &mut DatabaseState, op: &WalOp) -> Result<()> {
    match op {
        WalOp::CreateTable {
            name,
            columns,
            checks,
        } => {
            state
                .catalog
                .create_table(name.clone(), columns.clone(), checks.clone())?;
            state
                .tables
                .insert(normalize(name), HeapTable { rows: Vec::new() });
        }
        WalOp::DropTable { name } => {
            let removed = state.catalog.drop_table(name)?;
            state.tables.remove(&normalize(name));
            for idx in removed {
                state.indexes.remove(&normalize(&idx));
            }
        }
        WalOp::CreateIndex {
            name,
            table,
            column,
        } => {
            state
                .catalog
                .create_index(name.clone(), table.clone(), column.clone())?;
        }
        WalOp::DropIndex { name } => {
            state.catalog.drop_index(name)?;
            state.indexes.remove(&normalize(name));
        }
        WalOp::Insert {
            table,
            row_id,
            values,
        } => {
            state.next_row_id = state.next_row_id.max(row_id + 1);
            state
                .tables
                .entry(normalize(table))
                .or_insert_with(|| HeapTable { rows: Vec::new() })
                .rows
                .push(StoredRow {
                    id: *row_id,
                    values: values.clone(),
                });
        }
        WalOp::Update {
            table,
            row_id,
            values,
        } => {
            let rows = state.table_rows_mut(table)?;
            let row = rows
                .iter_mut()
                .find(|r| r.id == *row_id)
                .ok_or_else(|| MiniDbError::Storage(format!("unknown row id {row_id}")))?;
            row.values = values.clone();
        }
        WalOp::Delete { table, row_id } => {
            let rows = state.table_rows_mut(table)?;
            rows.retain(|r| r.id != *row_id);
        }
    }
    Ok(())
}

fn committed_ops(records: &[WalRecord]) -> BTreeMap<u64, Vec<&WalOp>> {
    let mut pending: BTreeMap<u64, Vec<&WalOp>> = BTreeMap::new();
    let mut committed = BTreeMap::new();
    for record in records {
        match record {
            WalRecord::Begin { tx } => {
                pending.entry(*tx).or_default();
            }
            WalRecord::Operation { tx, op } => {
                pending.entry(*tx).or_default().push(op);
            }
            WalRecord::Commit { tx } => {
                if let Some(ops) = pending.remove(tx) {
                    committed.insert(*tx, ops);
                }
            }
            WalRecord::Rollback { tx } => {
                pending.remove(tx);
            }
        }
    }
    committed
}

fn read_checkpoint(path: &Path) -> Result<DatabaseState> {
    let bytes = fs::read(path)?;
    if bytes.is_empty() {
        return Ok(DatabaseState::default());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_checkpoint(path: &Path, state: &DatabaseState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("db.tmp");
    let data = serde_json::to_vec_pretty(state)?;
    fs::write(&temp, data)?;
    fs::rename(temp, path)?;
    Ok(())
}

fn wal_path_for(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".wal");
    PathBuf::from(os)
}

pub fn next_row_id(state: &mut DatabaseState) -> u64 {
    let id = state.next_row_id;
    state.next_row_id += 1;
    id
}

pub fn column_names(columns: &[ColumnDef]) -> Vec<String> {
    columns.iter().map(|c| c.name.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::ColumnDef;
    use crate::types::{DataType, Value};
    use tempfile::tempdir;

    #[test]
    fn persists_insert_through_checkpoint() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut db = Database::open(&path).unwrap();
        db.begin().unwrap();
        db.append_and_apply(WalOp::CreateTable {
            name: "t".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                data_type: DataType::Int,
                primary_key: false,
                not_null: false,
                unique: false,
                default: None,
            }],
            checks: Vec::new(),
        })
        .unwrap();
        db.append_and_apply(WalOp::Insert {
            table: "t".into(),
            row_id: 1,
            values: vec![Value::Int(1)],
        })
        .unwrap();
        db.commit().unwrap();
        let db = Database::open(&path).unwrap();
        assert_eq!(db.state.table_rows("t").unwrap().len(), 1);
    }
}

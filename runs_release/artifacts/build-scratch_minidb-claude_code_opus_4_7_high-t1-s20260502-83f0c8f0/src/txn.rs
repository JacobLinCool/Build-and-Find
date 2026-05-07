//! Transaction manager.
//!
//! `minidb` runs a strict single-writer model: at most one transaction is
//! ever open. The manager tracks the transaction id, the in-memory buffer
//! of pending logical operations, and the WAL framing.
//!
//! Commit ordering (write-ahead):
//!   1. Append all logical operations to the WAL.
//!   2. Append a `Commit` record and fsync the WAL.
//!   3. Apply operations to the data file (heap + b-tree + catalog).
//!   4. Fsync the data file.
//!   5. Truncate the WAL.
//!
//! A crash between steps 2 and 5 is recovered by replaying the WAL on
//! startup; a crash before step 2 leaves the buffered work in memory only,
//! and the on-disk state is unchanged.

use crate::error::{MiniDbError, Result};
use crate::storage::page::RowId;
use crate::storage::wal::{WalRecord, WalWriter};
use crate::types::{ColumnDef, Value};
use std::collections::{HashMap, HashSet};

/// Pending operations recorded against a single transaction.
#[derive(Debug, Default)]
pub struct TxnBuffer {
    pub inserts: HashMap<String, Vec<Vec<Value>>>,
    pub deletes: HashMap<String, HashSet<RowId>>,
    pub updates: HashMap<String, HashMap<RowId, Vec<Value>>>,
    pub ddl: Vec<DdlOp>,
    pub wal_records: Vec<WalRecord>,
}

#[derive(Debug, Clone)]
pub enum DdlOp {
    CreateTable {
        name: String,
        columns: Vec<ColumnDef>,
    },
    DropTable {
        name: String,
    },
    CreateIndex {
        name: String,
        table: String,
        column: String,
    },
    DropIndex {
        name: String,
    },
}

impl TxnBuffer {
    pub fn new() -> Self {
        TxnBuffer::default()
    }

    pub fn record_insert(&mut self, table: &str, row: Vec<Value>, rec: WalRecord) {
        self.inserts
            .entry(table.to_ascii_lowercase())
            .or_default()
            .push(row);
        self.wal_records.push(rec);
    }

    pub fn record_delete(&mut self, table: &str, rid: RowId, rec: WalRecord) {
        self.deletes
            .entry(table.to_ascii_lowercase())
            .or_default()
            .insert(rid);
        self.wal_records.push(rec);
    }

    pub fn record_update(&mut self, table: &str, rid: RowId, new_row: Vec<Value>, rec: WalRecord) {
        self.updates
            .entry(table.to_ascii_lowercase())
            .or_default()
            .insert(rid, new_row);
        self.wal_records.push(rec);
    }

    pub fn record_ddl(&mut self, op: DdlOp, rec: WalRecord) {
        self.ddl.push(op);
        self.wal_records.push(rec);
    }

    pub fn is_empty(&self) -> bool {
        self.inserts.is_empty()
            && self.deletes.is_empty()
            && self.updates.is_empty()
            && self.ddl.is_empty()
            && self.wal_records.is_empty()
    }
}

/// State of the transaction manager.
#[derive(Debug)]
pub enum TxnState {
    AutoCommit,
    InTransaction { txn_id: u64 },
}

#[derive(Debug)]
pub struct TxnManager {
    next_txn_id: u64,
    state: TxnState,
}

impl TxnManager {
    pub fn new() -> Self {
        TxnManager {
            next_txn_id: 1,
            state: TxnState::AutoCommit,
        }
    }

    pub fn current_txn_id(&self) -> Option<u64> {
        match self.state {
            TxnState::AutoCommit => None,
            TxnState::InTransaction { txn_id } => Some(txn_id),
        }
    }

    pub fn allocate_id(&mut self) -> u64 {
        let id = self.next_txn_id;
        self.next_txn_id += 1;
        id
    }

    pub fn in_transaction(&self) -> bool {
        matches!(self.state, TxnState::InTransaction { .. })
    }

    pub fn begin(&mut self, wal: &mut WalWriter) -> Result<u64> {
        if self.in_transaction() {
            return Err(MiniDbError::txn("transaction already open"));
        }
        let id = self.allocate_id();
        wal.append(&WalRecord::Begin { txn_id: id })?;
        self.state = TxnState::InTransaction { txn_id: id };
        Ok(id)
    }

    pub fn end(&mut self) {
        self.state = TxnState::AutoCommit;
    }
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
        p.push(format!("minidb_txn_{}_{}.log", std::process::id(), n));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn begin_increments_id() {
        let mut tm = TxnManager::new();
        let p = tmp();
        let mut wal = WalWriter::open(&p).unwrap();
        let id = tm.begin(&mut wal).unwrap();
        assert_eq!(id, 1);
        assert!(tm.in_transaction());
        tm.end();
        let id2 = tm.begin(&mut wal).unwrap();
        assert_eq!(id2, 2);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn nested_begin_rejected() {
        let mut tm = TxnManager::new();
        let p = tmp();
        let mut wal = WalWriter::open(&p).unwrap();
        tm.begin(&mut wal).unwrap();
        assert!(tm.begin(&mut wal).is_err());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn buffer_records() {
        let mut buf = TxnBuffer::new();
        buf.record_insert(
            "t",
            vec![Value::Int(1)],
            WalRecord::Insert {
                txn_id: 1,
                table: "t".into(),
                values: vec![Value::Int(1)],
            },
        );
        assert_eq!(buf.inserts.get("t").unwrap().len(), 1);
        assert_eq!(buf.wal_records.len(), 1);
    }
}

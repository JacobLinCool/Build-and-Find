use crate::error::{MiniDbError, Result};
use crate::storage::wal::{Wal, WalRecord};
use std::collections::HashMap;

/// Undo log entry for rollback
#[derive(Debug, Clone)]
pub enum UndoEntry {
    InsertRow {
        table: String,
        row_id: u64,
    },
    DeleteRow {
        table: String,
        row_id: u64,
        data: Vec<u8>,
    },
    UpdateRow {
        table: String,
        row_id: u64,
        old_data: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TxnState {
    Active,
    Committed,
    Aborted,
}

pub struct Transaction {
    pub id: u64,
    pub state: TxnState,
    pub undo_log: Vec<UndoEntry>,
}

/// Single-writer transaction manager
pub struct TxnManager {
    next_txn_id: u64,
    active: Option<Transaction>,
}

impl TxnManager {
    pub fn new() -> Self {
        TxnManager {
            next_txn_id: 1,
            active: None,
        }
    }

    pub fn begin(&mut self, wal: &mut Wal) -> Result<u64> {
        if self.active.is_some() {
            return Err(MiniDbError::Transaction(
                "transaction already active".into(),
            ));
        }
        let txn_id = self.next_txn_id;
        self.next_txn_id += 1;
        wal.append(&WalRecord::Begin { txn_id })?;
        self.active = Some(Transaction {
            id: txn_id,
            state: TxnState::Active,
            undo_log: Vec::new(),
        });
        Ok(txn_id)
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn active_id(&self) -> Option<u64> {
        self.active.as_ref().map(|t| t.id)
    }

    pub fn log_insert(&mut self, table: &str, row_id: u64) {
        if let Some(txn) = &mut self.active {
            txn.undo_log.push(UndoEntry::InsertRow {
                table: table.into(),
                row_id,
            });
        }
    }

    pub fn log_delete(&mut self, table: &str, row_id: u64, data: Vec<u8>) {
        if let Some(txn) = &mut self.active {
            txn.undo_log.push(UndoEntry::DeleteRow {
                table: table.into(),
                row_id,
                data,
            });
        }
    }

    pub fn log_update(&mut self, table: &str, row_id: u64, old_data: Vec<u8>) {
        if let Some(txn) = &mut self.active {
            txn.undo_log.push(UndoEntry::UpdateRow {
                table: table.into(),
                row_id,
                old_data,
            });
        }
    }

    pub fn commit(&mut self, wal: &mut Wal) -> Result<u64> {
        let txn = self
            .active
            .as_ref()
            .ok_or_else(|| MiniDbError::Transaction("no active transaction".into()))?;
        let txn_id = txn.id;
        wal.append(&WalRecord::Commit { txn_id })?;
        wal.sync()?;
        self.active = None;
        Ok(txn_id)
    }

    pub fn rollback(&mut self, wal: &mut Wal) -> Result<Vec<UndoEntry>> {
        let txn = self
            .active
            .take()
            .ok_or_else(|| MiniDbError::Transaction("no active transaction".into()))?;
        wal.append(&WalRecord::Rollback { txn_id: txn.id })?;
        wal.flush()?;
        Ok(txn.undo_log)
    }

    /// Bump txn ID to at least `min` (used during WAL replay)
    pub fn set_min_txn_id(&mut self, min: u64) {
        if self.next_txn_id <= min {
            self.next_txn_id = min + 1;
        }
    }
}

impl Default for TxnManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::wal::Wal;
    use tempfile::NamedTempFile;

    #[test]
    fn test_begin_commit() {
        let tmp = NamedTempFile::new().unwrap();
        let mut wal = Wal::open(tmp.path()).unwrap();
        let mut mgr = TxnManager::new();
        let id = mgr.begin(&mut wal).unwrap();
        assert!(id > 0);
        assert!(mgr.is_active());
        mgr.commit(&mut wal).unwrap();
        assert!(!mgr.is_active());
    }

    #[test]
    fn test_double_begin_error() {
        let tmp = NamedTempFile::new().unwrap();
        let mut wal = Wal::open(tmp.path()).unwrap();
        let mut mgr = TxnManager::new();
        mgr.begin(&mut wal).unwrap();
        assert!(mgr.begin(&mut wal).is_err());
    }

    #[test]
    fn test_rollback_returns_undo() {
        let tmp = NamedTempFile::new().unwrap();
        let mut wal = Wal::open(tmp.path()).unwrap();
        let mut mgr = TxnManager::new();
        mgr.begin(&mut wal).unwrap();
        mgr.log_insert("t", 1);
        let undo = mgr.rollback(&mut wal).unwrap();
        assert_eq!(undo.len(), 1);
        matches!(undo[0], UndoEntry::InsertRow { .. });
    }
}

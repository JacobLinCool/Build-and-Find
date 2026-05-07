use crate::error::{MiniDbError, Result};
use crate::storage::heap::RowId;
use crate::storage::wal::{WalRecord, WalRecordBody, WalWriter};
use crate::types::Row;

pub type TxnId = u64;

#[derive(Debug, Clone, PartialEq)]
pub enum TxnState {
    Idle,
    Active(TxnId),
}

/// A pending WAL operation within a transaction (for buffering before commit).
#[derive(Debug, Clone)]
pub enum TxnOp {
    Insert {
        table: String,
        row_id: RowId,
        row: Row,
    },
    Delete {
        table: String,
        row_id: RowId,
        row: Row,
    },
    Update {
        table: String,
        row_id: RowId,
        new_row_id: RowId,
        before: Row,
        after: Row,
    },
    IndexInsert {
        index: String,
        key_bytes: Vec<u8>,
        row_id: RowId,
    },
    IndexDelete {
        index: String,
        key_bytes: Vec<u8>,
        row_id: RowId,
    },
}

/// Transaction manager. Single-writer model.
pub struct TxnManager {
    next_txn_id: TxnId,
    pub state: TxnState,
    pending_ops: Vec<TxnOp>,
}

impl TxnManager {
    pub fn new() -> Self {
        TxnManager {
            next_txn_id: 1,
            state: TxnState::Idle,
            pending_ops: Vec::new(),
        }
    }

    pub fn begin(&mut self) -> Result<TxnId> {
        match &self.state {
            TxnState::Active(_) => Err(MiniDbError::TxnError("transaction already active".into())),
            TxnState::Idle => {
                let id = self.next_txn_id;
                self.next_txn_id += 1;
                self.state = TxnState::Active(id);
                self.pending_ops.clear();
                Ok(id)
            }
        }
    }

    pub fn current_txn(&self) -> Option<TxnId> {
        match &self.state {
            TxnState::Active(id) => Some(*id),
            TxnState::Idle => None,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self.state, TxnState::Active(_))
    }

    /// Begin an implicit (auto-commit) transaction if none is active.
    pub fn begin_implicit(&mut self) -> TxnId {
        match &self.state {
            TxnState::Active(id) => *id,
            TxnState::Idle => {
                let id = self.next_txn_id;
                self.next_txn_id += 1;
                self.state = TxnState::Active(id);
                self.pending_ops.clear();
                id
            }
        }
    }

    pub fn log_op(&mut self, op: TxnOp) {
        self.pending_ops.push(op);
    }

    /// Commit the current transaction: write WAL and flush.
    /// Returns whether this was an auto-commit (for callers).
    pub fn commit(&mut self, wal: &mut WalWriter) -> Result<()> {
        let txn_id = match &self.state {
            TxnState::Active(id) => *id,
            TxnState::Idle => return Err(MiniDbError::TxnError("no active transaction".into())),
        };

        // Write begin record
        wal.write_record(&WalRecord {
            txn_id,
            body: WalRecordBody::Begin,
        })?;

        // Write all pending ops
        let ops = std::mem::take(&mut self.pending_ops);
        for op in &ops {
            let body = match op {
                TxnOp::Insert { table, row_id, row } => WalRecordBody::Insert {
                    table: table.clone(),
                    row_id: *row_id,
                    row: row.clone(),
                },
                TxnOp::Delete { table, row_id, row } => WalRecordBody::Delete {
                    table: table.clone(),
                    row_id: *row_id,
                    row: row.clone(),
                },
                TxnOp::Update {
                    table,
                    row_id,
                    new_row_id,
                    before,
                    after,
                } => WalRecordBody::Update {
                    table: table.clone(),
                    row_id: *row_id,
                    before: before.clone(),
                    after: after.clone(),
                    new_row_id: *new_row_id,
                },
                TxnOp::IndexInsert {
                    index,
                    key_bytes,
                    row_id,
                } => WalRecordBody::IndexInsert {
                    index: index.clone(),
                    key_bytes: key_bytes.clone(),
                    row_id: *row_id,
                },
                TxnOp::IndexDelete {
                    index,
                    key_bytes,
                    row_id,
                } => WalRecordBody::IndexDelete {
                    index: index.clone(),
                    key_bytes: key_bytes.clone(),
                    row_id: *row_id,
                },
            };
            wal.write_record(&WalRecord { txn_id, body })?;
        }

        // Write commit record
        wal.write_record(&WalRecord {
            txn_id,
            body: WalRecordBody::Commit,
        })?;
        wal.flush()?;

        self.state = TxnState::Idle;
        Ok(())
    }

    /// Rollback: discard pending ops.
    pub fn rollback(&mut self, wal: &mut WalWriter) -> Result<()> {
        let txn_id = match &self.state {
            TxnState::Active(id) => *id,
            TxnState::Idle => return Err(MiniDbError::TxnError("no active transaction".into())),
        };

        wal.write_record(&WalRecord {
            txn_id,
            body: WalRecordBody::Rollback,
        })?;
        wal.flush()?;

        self.pending_ops.clear();
        self.state = TxnState::Idle;
        Ok(())
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
    use crate::storage::wal::WalWriter;
    use tempfile::NamedTempFile;

    #[test]
    fn test_txn_begin_commit() {
        let tmp = NamedTempFile::new().unwrap();
        let mut wal = WalWriter::open(tmp.path()).unwrap();
        let mut tm = TxnManager::new();

        let id = tm.begin().unwrap();
        assert!(tm.is_active());
        tm.commit(&mut wal).unwrap();
        assert!(!tm.is_active());

        // double begin
        tm.begin().unwrap();
        assert!(tm.begin().is_err());
    }

    #[test]
    fn test_txn_rollback() {
        let tmp = NamedTempFile::new().unwrap();
        let mut wal = WalWriter::open(tmp.path()).unwrap();
        let mut tm = TxnManager::new();

        tm.begin().unwrap();
        tm.rollback(&mut wal).unwrap();
        assert!(!tm.is_active());
    }
}

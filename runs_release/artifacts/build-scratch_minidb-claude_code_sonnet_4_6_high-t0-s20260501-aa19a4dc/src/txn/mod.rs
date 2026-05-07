/// Transaction manager: single-writer model with WAL framing.
///
/// Provides explicit begin/commit/rollback. Auto-commit wraps individual statements.
use crate::error::{MiniDbError, Result};
use crate::storage::wal::{Wal, WalRecord};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    Idle,
    Active,
}

pub struct TransactionManager {
    wal: Wal,
    state: TxnState,
    current_txn_id: u64,
    next_txn_id: u64,
    pending_records: Vec<WalRecord>,
}

impl TransactionManager {
    pub fn new(wal: Wal) -> Self {
        TransactionManager {
            wal,
            state: TxnState::Idle,
            current_txn_id: 0,
            next_txn_id: 1,
            pending_records: Vec::new(),
        }
    }

    pub fn wal(&self) -> &Wal {
        &self.wal
    }

    pub fn current_txn_id(&self) -> u64 {
        self.current_txn_id
    }

    pub fn is_active(&self) -> bool {
        self.state == TxnState::Active
    }

    /// Begin a new transaction. Returns error if one is already active.
    pub fn begin(&mut self) -> Result<u64> {
        if self.state == TxnState::Active {
            return Err(MiniDbError::Transaction(
                "transaction already in progress".into(),
            ));
        }
        self.current_txn_id = self.next_txn_id;
        self.next_txn_id += 1;
        self.state = TxnState::Active;
        self.pending_records.clear();
        self.wal.append(&WalRecord::Begin {
            txn_id: self.current_txn_id,
        })?;
        log::debug!("begin txn {}", self.current_txn_id);
        Ok(self.current_txn_id)
    }

    /// Begin an implicit (auto-commit) transaction
    pub fn begin_auto(&mut self) -> Result<u64> {
        self.begin()
    }

    /// Log a data operation to the WAL buffer
    pub fn log_record(&mut self, record: WalRecord) -> Result<()> {
        if self.state != TxnState::Active {
            return Err(MiniDbError::Transaction("no active transaction".into()));
        }
        self.wal.append(&record)?;
        self.pending_records.push(record);
        Ok(())
    }

    /// Commit the current transaction
    pub fn commit(&mut self) -> Result<()> {
        if self.state != TxnState::Active {
            return Err(MiniDbError::Transaction("no active transaction".into()));
        }
        self.wal.append(&WalRecord::Commit {
            txn_id: self.current_txn_id,
        })?;
        self.wal.sync()?;
        log::debug!("commit txn {}", self.current_txn_id);
        self.state = TxnState::Idle;
        self.pending_records.clear();
        Ok(())
    }

    /// Rollback the current transaction
    pub fn rollback(&mut self) -> Result<Vec<WalRecord>> {
        if self.state != TxnState::Active {
            return Err(MiniDbError::Transaction("no active transaction".into()));
        }
        self.wal.append(&WalRecord::Rollback {
            txn_id: self.current_txn_id,
        })?;
        self.wal.sync()?;
        log::debug!("rollback txn {}", self.current_txn_id);
        self.state = TxnState::Idle;
        let records = std::mem::take(&mut self.pending_records);
        Ok(records)
    }

    pub fn wal_mut(&mut self) -> &mut Wal {
        &mut self.wal
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
        let wal = Wal::open(tmp.path()).unwrap();
        let mut txm = TransactionManager::new(wal);

        let tid = txm.begin().unwrap();
        assert!(txm.is_active());
        txm.commit().unwrap();
        assert!(!txm.is_active());
    }

    #[test]
    fn test_rollback() {
        let tmp = NamedTempFile::new().unwrap();
        let wal = Wal::open(tmp.path()).unwrap();
        let mut txm = TransactionManager::new(wal);

        txm.begin().unwrap();
        txm.log_record(WalRecord::InsertRow {
            txn_id: txm.current_txn_id(),
            table: "t".into(),
            row_id: 1,
            data: vec![],
        })
        .unwrap();
        let rolled_back = txm.rollback().unwrap();
        assert_eq!(rolled_back.len(), 1);
        assert!(!txm.is_active());
    }

    #[test]
    fn test_double_begin_fails() {
        let tmp = NamedTempFile::new().unwrap();
        let wal = Wal::open(tmp.path()).unwrap();
        let mut txm = TransactionManager::new(wal);

        txm.begin().unwrap();
        assert!(txm.begin().is_err());
    }
}

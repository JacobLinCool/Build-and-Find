use crate::error::{MiniDbError, Result};
use crate::storage::Wal;
use log::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    None,
    Active,
    Committed,
    Aborted,
}

pub struct TransactionManager {
    wal: Wal,
    current_txn_id: u64,
    state: TxnState,
    auto_commit: bool,
    in_explicit_txn: bool,
}

impl TransactionManager {
    pub fn new(wal: Wal) -> Self {
        Self {
            wal,
            current_txn_id: 0,
            state: TxnState::None,
            auto_commit: true,
            in_explicit_txn: false,
        }
    }

    pub fn wal(&mut self) -> &mut Wal {
        &mut self.wal
    }

    pub fn current_txn_id(&self) -> u64 {
        self.current_txn_id
    }

    pub fn state(&self) -> TxnState {
        self.state
    }

    pub fn in_transaction(&self) -> bool {
        self.in_explicit_txn
    }

    /// Begin a new explicit transaction.
    pub fn begin(&mut self) -> Result<()> {
        if self.in_explicit_txn {
            return Err(MiniDbError::Transaction(
                "cannot begin a transaction within an active transaction".into(),
            ));
        }

        self.current_txn_id = self.wal.next_txn_id();
        self.state = TxnState::Active;
        self.auto_commit = false;
        self.in_explicit_txn = true;

        self.wal.append_begin(self.current_txn_id)?;
        debug!("BEGIN transaction {}", self.current_txn_id);
        Ok(())
    }

    /// Start an implicit auto-commit transaction.
    pub fn begin_auto(&mut self) -> Result<()> {
        if self.in_explicit_txn {
            return Ok(()); // already in a transaction
        }

        self.current_txn_id = self.wal.next_txn_id();
        self.state = TxnState::Active;
        self.auto_commit = true;

        self.wal.append_begin(self.current_txn_id)?;
        debug!("BEGIN auto-commit transaction {}", self.current_txn_id);
        Ok(())
    }

    /// Commit the current transaction.
    pub fn commit(&mut self) -> Result<()> {
        if self.state != TxnState::Active {
            return Err(MiniDbError::Transaction(
                "no active transaction to commit".into(),
            ));
        }

        self.wal.append_commit(self.current_txn_id)?;
        self.wal.sync()?;
        self.state = TxnState::Committed;
        self.in_explicit_txn = false;
        self.auto_commit = true;

        debug!("COMMIT transaction {}", self.current_txn_id);
        Ok(())
    }

    /// Rollback the current transaction.
    pub fn rollback(&mut self) -> Result<()> {
        if !self.in_explicit_txn && self.state != TxnState::Active {
            return Err(MiniDbError::Transaction(
                "no active transaction to rollback".into(),
            ));
        }

        self.state = TxnState::Aborted;
        self.in_explicit_txn = false;
        self.auto_commit = true;

        debug!("ROLLBACK transaction {}", self.current_txn_id);
        Ok(())
    }

    /// Auto-commit if we're in an implicit transaction.
    pub fn auto_commit_if_needed(&mut self) -> Result<()> {
        if self.auto_commit && self.state == TxnState::Active {
            self.commit()?;
        }
        Ok(())
    }

    /// Write a page to the WAL.
    pub fn log_page_write(&mut self, page_id: u64, data: &[u8]) -> Result<()> {
        self.wal
            .append_page_write(self.current_txn_id, page_id, data)
    }

    /// Write catalog update to the WAL.
    pub fn log_catalog_update(&mut self, data: &[u8]) -> Result<()> {
        self.wal.append_catalog_update(self.current_txn_id, data)
    }

    /// Replay the WAL for crash recovery.
    pub fn replay_wal(&mut self) -> Result<crate::storage::WalReplayResult> {
        self.wal.replay()
    }

    /// Truncate the WAL after checkpoint.
    pub fn checkpoint(&mut self) -> Result<()> {
        self.wal.append_checkpoint()?;
        self.wal.truncate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_txn_manager() -> (TransactionManager, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");
        let wal = Wal::create(wal_path.to_str().unwrap()).unwrap();
        (TransactionManager::new(wal), dir)
    }

    #[test]
    fn test_begin_commit() {
        let (mut txn, _dir) = create_txn_manager();
        txn.begin().unwrap();
        assert_eq!(txn.state(), TxnState::Active);
        assert!(txn.in_transaction());

        txn.commit().unwrap();
        assert_eq!(txn.state(), TxnState::Committed);
        assert!(!txn.in_transaction());
    }

    #[test]
    fn test_begin_rollback() {
        let (mut txn, _dir) = create_txn_manager();
        txn.begin().unwrap();
        txn.rollback().unwrap();
        assert_eq!(txn.state(), TxnState::Aborted);
        assert!(!txn.in_transaction());
    }

    #[test]
    fn test_nested_begin_error() {
        let (mut txn, _dir) = create_txn_manager();
        txn.begin().unwrap();
        assert!(txn.begin().is_err());
    }

    #[test]
    fn test_auto_commit() {
        let (mut txn, _dir) = create_txn_manager();
        txn.begin_auto().unwrap();
        assert_eq!(txn.state(), TxnState::Active);

        txn.auto_commit_if_needed().unwrap();
        assert_eq!(txn.state(), TxnState::Committed);
    }

    #[test]
    fn test_commit_within_explicit_txn() {
        let (mut txn, _dir) = create_txn_manager();
        txn.begin().unwrap();
        txn.begin_auto().unwrap(); // should be no-op
        assert!(txn.in_transaction());
        txn.commit().unwrap();
    }
}

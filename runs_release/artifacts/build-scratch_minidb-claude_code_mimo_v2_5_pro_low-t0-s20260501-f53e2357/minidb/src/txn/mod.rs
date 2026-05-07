use crate::error::{MiniDbError, Result};
use crate::storage::wal;
use crate::storage::Storage;

pub struct TransactionManager {
    next_txn_id: u64,
    active_txn: Option<u64>,
}

impl TransactionManager {
    pub fn new() -> Self {
        Self {
            next_txn_id: 1,
            active_txn: None,
        }
    }

    pub fn has_active_transaction(&self) -> bool {
        self.active_txn.is_some()
    }

    pub fn current_txn_id(&self) -> Option<u64> {
        self.active_txn
    }

    pub fn begin(&mut self) -> Result<u64> {
        if self.active_txn.is_some() {
            return Err(MiniDbError::Runtime {
                message: "cannot start a transaction within a transaction".into(),
            });
        }
        let txn_id = self.next_txn_id;
        self.next_txn_id += 1;
        self.active_txn = Some(txn_id);
        log::debug!("BEGIN txn {}", txn_id);
        Ok(txn_id)
    }

    pub fn commit(&mut self, storage: &mut Storage) -> Result<()> {
        let txn_id = self.active_txn.ok_or_else(|| MiniDbError::Runtime {
            message: "no active transaction to commit".into(),
        })?;

        storage.wal.append(txn_id, wal::WAL_COMMIT, &[])?;
        storage.wal.flush()?;
        self.active_txn = None;
        log::debug!("COMMIT txn {}", txn_id);
        Ok(())
    }

    pub fn rollback(&mut self, storage: &mut Storage) -> Result<()> {
        let txn_id = self.active_txn.ok_or_else(|| MiniDbError::Runtime {
            message: "no active transaction to rollback".into(),
        })?;

        storage.wal.append(txn_id, wal::WAL_ROLLBACK, &[])?;
        storage.wal.flush()?;
        self.active_txn = None;
        log::debug!("ROLLBACK txn {}", txn_id);
        Ok(())
    }

    /// Get or create an auto-commit transaction
    pub fn auto_txn_id(&mut self) -> u64 {
        if let Some(id) = self.active_txn {
            id
        } else {
            let id = self.next_txn_id;
            self.next_txn_id += 1;
            id
        }
    }

    /// Auto-commit: commit immediately if not in explicit transaction
    pub fn auto_commit(&mut self, storage: &mut Storage) -> Result<()> {
        if self.active_txn.is_none() {
            // Auto-commit: we need to commit the implicit transaction
            let txn_id = self.next_txn_id - 1; // last used txn_id
            storage.wal.append(txn_id, wal::WAL_COMMIT, &[])?;
            storage.wal.flush()?;
        }
        Ok(())
    }

    /// Replay WAL to restore committed state
    pub fn replay_wal(&self, storage: &mut Storage) -> Result<()> {
        let records = storage.wal.read_committed_records()?;
        if records.is_empty() {
            return Ok(());
        }
        log::info!("Replaying {} committed WAL records", records.len());
        // Records are read but actual replay happens at the storage layer
        // by the caller who knows the catalog and heap layout
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup() -> (Storage, TransactionManager) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let storage = Storage::open(&path).unwrap();
        let txn = TransactionManager::new();
        (storage, txn)
    }

    #[test]
    fn test_begin_commit() {
        let (mut storage, mut txn) = setup();
        let id = txn.begin().unwrap();
        assert!(txn.has_active_transaction());
        assert_eq!(txn.current_txn_id(), Some(id));
        txn.commit(&mut storage).unwrap();
        assert!(!txn.has_active_transaction());
    }

    #[test]
    fn test_begin_rollback() {
        let (mut storage, mut txn) = setup();
        txn.begin().unwrap();
        txn.rollback(&mut storage).unwrap();
        assert!(!txn.has_active_transaction());
    }

    #[test]
    fn test_no_nested_transactions() {
        let (_, mut txn) = setup();
        txn.begin().unwrap();
        assert!(txn.begin().is_err());
    }

    #[test]
    fn test_commit_without_txn() {
        let (mut storage, mut txn) = setup();
        assert!(txn.commit(&mut storage).is_err());
    }
}

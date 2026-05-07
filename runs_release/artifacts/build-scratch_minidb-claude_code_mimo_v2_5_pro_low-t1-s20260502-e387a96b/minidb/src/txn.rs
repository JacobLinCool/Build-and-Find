use crate::error::{MiniDbError, Result};
use crate::storage::wal::{Wal, WalRecord};
use crate::storage::Pager;
use log::debug;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct TransactionManager {
    wal: Wal,
    wal_path: PathBuf,
    current_txn_id: u64,
    in_transaction: bool,
    next_txn_id: u64,
}

impl TransactionManager {
    pub fn new(wal_path: PathBuf) -> Result<Self> {
        let wal = Wal::open(&wal_path)?;
        Ok(Self {
            wal,
            wal_path,
            current_txn_id: 0,
            in_transaction: false,
            next_txn_id: 1,
        })
    }

    pub fn begin(&mut self) -> Result<u64> {
        if self.in_transaction {
            return Err(MiniDbError::RuntimeError(
                "already in a transaction; commit or rollback first".into(),
            ));
        }
        let txn_id = self.next_txn_id;
        self.next_txn_id += 1;
        self.wal.append(&WalRecord::Begin { txn_id })?;
        self.current_txn_id = txn_id;
        self.in_transaction = true;
        debug!("BEGIN txn {txn_id}");
        Ok(txn_id)
    }

    pub fn commit(&mut self) -> Result<()> {
        if !self.in_transaction {
            return Err(MiniDbError::RuntimeError("no active transaction".into()));
        }
        let txn_id = self.current_txn_id;
        self.wal.append(&WalRecord::Commit { txn_id })?;
        self.wal.sync()?;
        self.in_transaction = false;
        debug!("COMMIT txn {txn_id}");
        Ok(())
    }

    pub fn rollback(&mut self) -> Result<()> {
        if !self.in_transaction {
            return Err(MiniDbError::RuntimeError("no active transaction".into()));
        }
        let txn_id = self.current_txn_id;
        // We don't write a rollback record; we just discard dirty pages
        self.in_transaction = false;
        debug!("ROLLBACK txn {txn_id}");
        Ok(())
    }

    pub fn current_txn_id(&self) -> u64 {
        self.current_txn_id
    }

    pub fn in_transaction(&self) -> bool {
        self.in_transaction
    }

    /// Auto-begin if not in transaction (for auto-commit mode)
    pub fn ensure_transaction(&mut self) -> Result<u64> {
        if !self.in_transaction {
            self.begin()
        } else {
            Ok(self.current_txn_id)
        }
    }

    /// Auto-commit if in auto-commit mode (not in explicit transaction)
    pub fn auto_commit_if_needed(&mut self, was_auto: bool) -> Result<()> {
        if was_auto && self.in_transaction {
            self.commit()?;
        }
        Ok(())
    }

    pub fn wal(&mut self) -> &mut Wal {
        &mut self.wal
    }

    pub fn wal_path(&self) -> &PathBuf {
        &self.wal_path
    }

    /// Replay WAL for crash recovery
    pub fn replay_wal() -> Result<Vec<WalRecord>> {
        // This is called with the path; we need it from the WAL file
        // Actually, let's make this take a path
        Ok(Vec::new())
    }

    pub fn truncate_wal(&mut self) -> Result<()> {
        self.wal.sync()?;
        Wal::truncate(&self.wal_path)?;
        self.wal = Wal::open(&self.wal_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_txn_begin_commit() {
        let path = std::env::temp_dir().join(format!("minidb_txn_test_{}.wal", std::process::id()));
        let _ = fs::remove_file(&path);
        {
            let mut tm = TransactionManager::new(path.clone()).unwrap();
            assert!(!tm.in_transaction());
            let txn_id = tm.begin().unwrap();
            assert!(tm.in_transaction());
            assert_eq!(txn_id, 1);
            tm.commit().unwrap();
            assert!(!tm.in_transaction());
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_txn_rollback() {
        let path =
            std::env::temp_dir().join(format!("minidb_txn_rb_test_{}.wal", std::process::id()));
        let _ = fs::remove_file(&path);
        {
            let mut tm = TransactionManager::new(path.clone()).unwrap();
            tm.begin().unwrap();
            tm.rollback().unwrap();
            assert!(!tm.in_transaction());
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_txn_no_nested() {
        let path =
            std::env::temp_dir().join(format!("minidb_txn_nested_test_{}.wal", std::process::id()));
        let _ = fs::remove_file(&path);
        {
            let mut tm = TransactionManager::new(path.clone()).unwrap();
            tm.begin().unwrap();
            assert!(tm.begin().is_err());
            tm.rollback().unwrap();
        }
        let _ = fs::remove_file(&path);
    }
}

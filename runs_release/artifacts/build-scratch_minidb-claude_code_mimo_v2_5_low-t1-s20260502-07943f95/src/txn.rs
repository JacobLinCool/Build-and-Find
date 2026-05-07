use crate::error::{MiniDbError, Result};
use crate::storage::wal::{Wal, WalRecord};
use log::debug;

#[derive(Debug, Clone)]
pub struct Transaction {
    pub id: u64,
    pub state: TxnState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TxnState {
    Active,
    Committed,
    Aborted,
}

pub struct TransactionManager {
    wal: Wal,
    active_txn: Option<Transaction>,
    next_tx_id: u64,
    auto_commit: bool,
}

impl TransactionManager {
    pub fn new(mut wal: Wal) -> Self {
        let next_tx_id = wal.next_tx_id();
        Self {
            wal,
            active_txn: None,
            next_tx_id,
            auto_commit: true,
        }
    }

    pub fn begin(&mut self) -> Result<u64> {
        if self.active_txn.is_some() {
            return Err(MiniDbError::Transaction(
                "Transaction already in progress".into(),
            ));
        }
        let tx_id = self.next_tx_id;
        self.next_tx_id += 1;
        self.wal.begin(tx_id)?;
        self.active_txn = Some(Transaction {
            id: tx_id,
            state: TxnState::Active,
        });
        self.auto_commit = false;
        debug!("Transaction {} started", tx_id);
        Ok(tx_id)
    }

    pub fn commit(&mut self) -> Result<u64> {
        let tx_id = match self.active_txn.take() {
            Some(tx) => {
                if tx.state != TxnState::Active {
                    return Err(MiniDbError::Transaction("Transaction not active".into()));
                }
                tx.id
            }
            None => return Err(MiniDbError::Transaction("No active transaction".into())),
        };
        self.wal.commit(tx_id)?;
        self.auto_commit = true;
        debug!("Transaction {} committed", tx_id);
        Ok(tx_id)
    }

    pub fn rollback(&mut self) -> Result<u64> {
        let tx_id = match self.active_txn.take() {
            Some(tx) => {
                if tx.state != TxnState::Active {
                    return Err(MiniDbError::Transaction("Transaction not active".into()));
                }
                tx.id
            }
            None => return Err(MiniDbError::Transaction("No active transaction".into())),
        };
        self.wal.rollback(tx_id)?;
        self.auto_commit = true;
        debug!("Transaction {} rolled back", tx_id);
        Ok(tx_id)
    }

    pub fn auto_begin(&mut self) -> Result<u64> {
        if self.auto_commit {
            self.begin()
        } else if let Some(ref txn) = self.active_txn {
            Ok(txn.id)
        } else {
            Err(MiniDbError::Transaction("No active transaction".into()))
        }
    }

    pub fn auto_commit(&mut self) -> Result<()> {
        if self.auto_commit && self.active_txn.is_some() {
            self.commit()?;
        }
        Ok(())
    }

    pub fn is_in_transaction(&self) -> bool {
        self.active_txn.is_some()
    }

    pub fn current_tx_id(&self) -> Option<u64> {
        self.active_txn.as_ref().map(|t| t.id)
    }

    pub fn replay_wal(&mut self) -> Result<Vec<WalRecord>> {
        self.wal.replay()
    }

    pub fn log_insert(
        &mut self,
        tx_id: u64,
        table: &str,
        row_id: u64,
        data: Vec<u8>,
    ) -> Result<()> {
        self.wal.log_insert(tx_id, table, row_id, data)
    }

    pub fn log_update(
        &mut self,
        tx_id: u64,
        table: &str,
        row_id: u64,
        old_data: Vec<u8>,
        new_data: Vec<u8>,
    ) -> Result<()> {
        self.wal
            .log_update(tx_id, table, row_id, old_data, new_data)
    }

    pub fn log_delete(&mut self, tx_id: u64, table: &str, row_id: u64) -> Result<()> {
        self.wal.log_delete(tx_id, table, row_id)
    }

    pub fn clear_wal(&mut self) -> Result<()> {
        self.wal.clear()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempfile;

    fn create_test_txn_mgr() -> TransactionManager {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wal");
        let path_str = path.to_str().unwrap();
        let wal = Wal::open(path_str).unwrap();
        TransactionManager::new(wal)
    }

    #[test]
    fn test_begin_commit() {
        let mut mgr = create_test_txn_mgr();
        let tx_id = mgr.begin().unwrap();
        assert!(mgr.is_in_transaction());
        assert_eq!(mgr.current_tx_id(), Some(tx_id));
        mgr.commit().unwrap();
        assert!(!mgr.is_in_transaction());
    }

    #[test]
    fn test_begin_rollback() {
        let mut mgr = create_test_txn_mgr();
        let tx_id = mgr.begin().unwrap();
        mgr.rollback().unwrap();
        assert!(!mgr.is_in_transaction());
    }

    #[test]
    fn test_double_begin_fails() {
        let mut mgr = create_test_txn_mgr();
        mgr.begin().unwrap();
        assert!(mgr.begin().is_err());
    }

    #[test]
    fn test_commit_without_begin() {
        let mut mgr = create_test_txn_mgr();
        assert!(mgr.commit().is_err());
    }
}

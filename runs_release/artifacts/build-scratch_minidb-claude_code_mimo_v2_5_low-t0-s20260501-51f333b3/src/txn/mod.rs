use crate::error::{MiniDbError, Result};
use log::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    AutoCommit,
    InTransaction,
}

pub struct TransactionManager {
    state: TxnState,
    txn_id: u64,
}

impl TransactionManager {
    pub fn new() -> Self {
        TransactionManager {
            state: TxnState::AutoCommit,
            txn_id: 0,
        }
    }

    pub fn begin(&mut self) -> Result<u64> {
        match self.state {
            TxnState::InTransaction => Err(MiniDbError::TransactionError(
                "transaction already in progress".to_string(),
            )),
            TxnState::AutoCommit => {
                self.txn_id += 1;
                self.state = TxnState::InTransaction;
                debug!("Transaction {} started", self.txn_id);
                Ok(self.txn_id)
            }
        }
    }

    pub fn commit(&mut self) -> Result<()> {
        match self.state {
            TxnState::AutoCommit => Err(MiniDbError::TransactionError(
                "no transaction in progress".to_string(),
            )),
            TxnState::InTransaction => {
                debug!("Transaction {} committed", self.txn_id);
                self.state = TxnState::AutoCommit;
                Ok(())
            }
        }
    }

    pub fn rollback(&mut self) -> Result<()> {
        match self.state {
            TxnState::AutoCommit => Err(MiniDbError::TransactionError(
                "no transaction in progress".to_string(),
            )),
            TxnState::InTransaction => {
                debug!("Transaction {} rolled back", self.txn_id);
                self.state = TxnState::AutoCommit;
                Ok(())
            }
        }
    }

    pub fn is_in_transaction(&self) -> bool {
        self.state == TxnState::InTransaction
    }

    pub fn txn_id(&self) -> u64 {
        self.txn_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_begin_commit() {
        let mut txn = TransactionManager::new();
        assert!(!txn.is_in_transaction());

        let id = txn.begin().unwrap();
        assert!(txn.is_in_transaction());
        assert_eq!(id, 1);

        txn.commit().unwrap();
        assert!(!txn.is_in_transaction());
    }

    #[test]
    fn test_begin_rollback() {
        let mut txn = TransactionManager::new();
        txn.begin().unwrap();
        txn.rollback().unwrap();
        assert!(!txn.is_in_transaction());
    }

    #[test]
    fn test_double_begin_error() {
        let mut txn = TransactionManager::new();
        txn.begin().unwrap();
        assert!(txn.begin().is_err());
    }

    #[test]
    fn test_commit_without_begin_error() {
        let mut txn = TransactionManager::new();
        assert!(txn.commit().is_err());
    }

    #[test]
    fn test_rollback_without_begin_error() {
        let mut txn = TransactionManager::new();
        assert!(txn.rollback().is_err());
    }
}

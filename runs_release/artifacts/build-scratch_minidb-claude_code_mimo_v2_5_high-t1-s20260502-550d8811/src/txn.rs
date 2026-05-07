use crate::error::{MiniDbError, Result};
use crate::storage::wal::{Wal, WalOp, WalRecord};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Transaction {
    pub id: u64,
    pub status: TxnStatus,
    pub operations: Vec<WalRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TxnStatus {
    Active,
    Committed,
    Aborted,
}

pub struct TransactionManager {
    next_tx_id: u64,
    active_tx: Option<u64>,
    transactions: HashMap<u64, Transaction>,
}

impl TransactionManager {
    pub fn new() -> Self {
        TransactionManager {
            next_tx_id: 1,
            active_tx: None,
            transactions: HashMap::new(),
        }
    }

    pub fn begin(&mut self) -> Result<u64> {
        if self.active_tx.is_some() {
            return Err(MiniDbError::Transaction(
                "nested transactions not supported".into(),
            ));
        }
        let tx_id = self.next_tx_id;
        self.next_tx_id += 1;
        self.transactions.insert(
            tx_id,
            Transaction {
                id: tx_id,
                status: TxnStatus::Active,
                operations: Vec::new(),
            },
        );
        self.active_tx = Some(tx_id);
        log::info!("Transaction {} started", tx_id);
        Ok(tx_id)
    }

    pub fn commit(&mut self, tx_id: u64, wal: &mut Wal) -> Result<()> {
        let tx = self.transactions.get_mut(&tx_id).ok_or_else(|| {
            MiniDbError::Transaction(format!("transaction {} does not exist", tx_id))
        })?;

        if tx.status != TxnStatus::Active {
            return Err(MiniDbError::Transaction(format!(
                "transaction {} is not active",
                tx_id
            )));
        }

        // Write commit record to WAL
        wal.append(WalOp::Commit { tx_id })?;

        tx.status = TxnStatus::Committed;
        if self.active_tx == Some(tx_id) {
            self.active_tx = None;
        }

        log::info!("Transaction {} committed", tx_id);
        Ok(())
    }

    pub fn rollback(&mut self, tx_id: u64, wal: &mut Wal) -> Result<()> {
        let tx = self.transactions.get_mut(&tx_id).ok_or_else(|| {
            MiniDbError::Transaction(format!("transaction {} does not exist", tx_id))
        })?;

        if tx.status != TxnStatus::Active {
            return Err(MiniDbError::Transaction(format!(
                "transaction {} is not active",
                tx_id
            )));
        }

        // Write rollback record to WAL
        wal.append(WalOp::Rollback { tx_id })?;

        tx.status = TxnStatus::Aborted;
        if self.active_tx == Some(tx_id) {
            self.active_tx = None;
        }

        log::info!("Transaction {} rolled back", tx_id);
        Ok(())
    }

    pub fn active_tx_id(&self) -> Option<u64> {
        self.active_tx
    }

    pub fn in_transaction(&self) -> bool {
        self.active_tx.is_some()
    }

    pub fn add_operation(&mut self, tx_id: u64, record: WalRecord) {
        if let Some(tx) = self.transactions.get_mut(&tx_id) {
            tx.operations.push(record);
        }
    }

    pub fn get_operations(&self, tx_id: u64) -> Vec<&WalRecord> {
        self.transactions
            .get(&tx_id)
            .map(|tx| tx.operations.iter().collect())
            .unwrap_or_default()
    }

    /// Replay WAL records to rebuild transaction state on startup
    pub fn replay_wal(&mut self, records: &[WalRecord]) {
        for record in records {
            match &record.op {
                WalOp::Begin { tx_id } => {
                    self.transactions.insert(
                        *tx_id,
                        Transaction {
                            id: *tx_id,
                            status: TxnStatus::Active,
                            operations: Vec::new(),
                        },
                    );
                    if *tx_id >= self.next_tx_id {
                        self.next_tx_id = tx_id + 1;
                    }
                }
                WalOp::Commit { tx_id } => {
                    if let Some(tx) = self.transactions.get_mut(tx_id) {
                        tx.status = TxnStatus::Committed;
                    }
                }
                WalOp::Rollback { tx_id } => {
                    if let Some(tx) = self.transactions.get_mut(tx_id) {
                        tx.status = TxnStatus::Aborted;
                    }
                }
                _ => {
                    // Record the operation in the transaction
                    if let Some(tx_id) = Self::tx_id_from_op(&record.op) {
                        if let Some(tx) = self.transactions.get_mut(&tx_id) {
                            tx.operations.push(record.clone());
                        }
                    }
                }
            }
        }
    }

    fn tx_id_from_op(op: &WalOp) -> Option<u64> {
        match op {
            WalOp::Begin { tx_id }
            | WalOp::Commit { tx_id }
            | WalOp::Rollback { tx_id }
            | WalOp::Insert { tx_id, .. }
            | WalOp::Update { tx_id, .. }
            | WalOp::Delete { tx_id, .. }
            | WalOp::CreateTable { tx_id, .. }
            | WalOp::DropTable { tx_id, .. }
            | WalOp::CreateIndex { tx_id, .. }
            | WalOp::DropIndex { tx_id, .. } => Some(*tx_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_begin_commit() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");
        let mut wal = Wal::open(&path).unwrap();
        let mut tm = TransactionManager::new();

        let tx_id = tm.begin().unwrap();
        assert_eq!(tx_id, 1);
        assert!(tm.in_transaction());

        tm.commit(tx_id, &mut wal).unwrap();
        assert!(!tm.in_transaction());
    }

    #[test]
    fn test_begin_rollback() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");
        let mut wal = Wal::open(&path).unwrap();
        let mut tm = TransactionManager::new();

        let tx_id = tm.begin().unwrap();
        tm.rollback(tx_id, &mut wal).unwrap();
        assert!(!tm.in_transaction());
    }

    #[test]
    fn test_no_nested_transactions() {
        let mut tm = TransactionManager::new();
        tm.begin().unwrap();
        assert!(tm.begin().is_err());
    }
}

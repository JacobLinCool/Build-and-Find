//! Transaction manager.
//!
//! Single-writer model: at most one transaction is active at a time. The
//! transaction collects WAL records into a buffer; on commit, the buffer is
//! flushed to the WAL with an explicit `Commit` record and the WAL is fsynced
//! before changes become observable. On rollback, the buffered records are
//! discarded and the in-memory state is reverted from the snapshot.

use crate::error::{MiniDbError, Result};
use crate::storage::wal::WalRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    Idle,
    Explicit, // user issued BEGIN
    AutoCommit,
}

#[derive(Debug)]
pub struct Transaction {
    pub id: u64,
    pub state: TxnState,
    pub records: Vec<WalRecord>,
}

impl Transaction {
    pub fn new(id: u64, state: TxnState) -> Self {
        Transaction {
            id,
            state,
            records: Vec::new(),
        }
    }
}

#[derive(Debug, Default)]
pub struct TxnManager {
    pub active: Option<Transaction>,
}

impl TxnManager {
    pub fn new() -> Self {
        TxnManager::default()
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn is_explicit(&self) -> bool {
        matches!(
            self.active.as_ref().map(|t| t.state),
            Some(TxnState::Explicit)
        )
    }

    pub fn begin(&mut self, id: u64, explicit: bool) -> Result<()> {
        if matches!(
            self.active.as_ref().map(|t| t.state),
            Some(TxnState::Explicit)
        ) {
            return Err(MiniDbError::Runtime("transaction already active".into()));
        }
        self.active = Some(Transaction::new(
            id,
            if explicit {
                TxnState::Explicit
            } else {
                TxnState::AutoCommit
            },
        ));
        Ok(())
    }

    pub fn record(&mut self, rec: WalRecord) -> Result<()> {
        let txn = self
            .active
            .as_mut()
            .ok_or_else(|| MiniDbError::Runtime("no active transaction".into()))?;
        txn.records.push(rec);
        Ok(())
    }

    pub fn take(&mut self) -> Option<Transaction> {
        self.active.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cannot_begin_twice() {
        let mut m = TxnManager::new();
        m.begin(1, true).unwrap();
        let err = m.begin(2, true);
        assert!(err.is_err());
    }

    #[test]
    fn record_requires_active_txn() {
        let mut m = TxnManager::new();
        assert!(m.record(WalRecord::Begin { txn_id: 1 }).is_err());
        m.begin(1, false).unwrap();
        assert!(m.record(WalRecord::Begin { txn_id: 1 }).is_ok());
    }
}

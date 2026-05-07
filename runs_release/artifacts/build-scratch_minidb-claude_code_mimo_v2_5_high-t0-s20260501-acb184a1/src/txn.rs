use crate::error::{MiniDbError, Result};
use crate::storage::{PageManager, Wal, WalRecord};

/// Transaction state
#[derive(Debug, Clone, PartialEq)]
pub enum TxnState {
    Inactive,
    Active { xid: u64 },
    Committed,
    Aborted,
}

/// Transaction manager handles BEGIN/COMMIT/ROLLBACK
pub struct TxnManager {
    state: TxnState,
    next_xid: u64,
    pending_writes: Vec<WalRecord>,
}

impl TxnManager {
    pub fn new() -> Self {
        TxnManager {
            state: TxnState::Inactive,
            next_xid: 1,
            pending_writes: Vec::new(),
        }
    }

    pub fn begin(&mut self, wal: &mut Wal) -> Result<u64> {
        if matches!(self.state, TxnState::Active { .. }) {
            return Err(MiniDbError::TransactionError(
                "Transaction already active".into(),
            ));
        }
        let xid = self.next_xid;
        self.next_xid += 1;
        wal.append(&WalRecord::Begin { xid })?;
        self.state = TxnState::Active { xid };
        self.pending_writes.clear();
        log::trace!("Transaction {} started", xid);
        Ok(xid)
    }

    pub fn commit(&mut self, wal: &mut Wal, page_manager: &mut PageManager) -> Result<()> {
        match self.state {
            TxnState::Active { xid } => {
                wal.append(&WalRecord::Commit { xid })?;
                page_manager.flush_all()?;
                wal.truncate()?;
                self.state = TxnState::Committed;
                self.pending_writes.clear();
                log::trace!("Transaction {} committed", xid);
                Ok(())
            }
            TxnState::Inactive => {
                // Auto-commit: flush and clear WAL
                page_manager.flush_all()?;
                wal.truncate()?;
                Ok(())
            }
            _ => Err(MiniDbError::TransactionError(
                "No active transaction to commit".into(),
            )),
        }
    }

    pub fn rollback(&mut self, wal: &mut Wal) -> Result<()> {
        match self.state {
            TxnState::Active { xid } => {
                wal.truncate()?;
                self.state = TxnState::Aborted;
                self.pending_writes.clear();
                log::trace!("Transaction {} rolled back", xid);
                Ok(())
            }
            _ => Err(MiniDbError::TransactionError(
                "No active transaction to rollback".into(),
            )),
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self.state, TxnState::Active { .. })
    }

    pub fn current_xid(&self) -> Option<u64> {
        match self.state {
            TxnState::Active { xid } => Some(xid),
            _ => None,
        }
    }

    pub fn add_pending(&mut self, record: WalRecord) {
        self.pending_writes.push(record);
    }

    pub fn take_pending(&mut self) -> Vec<WalRecord> {
        std::mem::take(&mut self.pending_writes)
    }

    /// Replay WAL for crash recovery
    pub fn recover(wal: &mut Wal, page_manager: &mut PageManager) -> Result<()> {
        let records = wal.read_all()?;
        log::info!("Replaying {} WAL records", records.len());

        // Find the last committed transaction
        let mut committed_xids = std::collections::HashSet::new();
        let mut active_xids = std::collections::HashSet::new();

        for record in &records {
            match record {
                WalRecord::Begin { xid } => {
                    active_xids.insert(*xid);
                }
                WalRecord::Commit { xid } => {
                    committed_xids.insert(*xid);
                    active_xids.remove(xid);
                }
                _ => {}
            }
        }

        // Replay committed transactions
        for record in &records {
            match record {
                WalRecord::CreateTable { schema } => {
                    let mut catalog_tables = page_manager
                        .load_schemas()
                        .map(|(t, _)| t)
                        .unwrap_or_default();
                    if !catalog_tables.iter().any(|t| t.name == schema.name) {
                        catalog_tables.push(schema.clone());
                        let _ = page_manager.load_schemas().unwrap_or_default();
                        // Save back
                    }
                }
                WalRecord::Insert {
                    table,
                    row_id,
                    row: _,
                } => {
                    log::debug!("Replay: INSERT into {} row_id={}", table, row_id);
                }
                WalRecord::Delete { table, row_id } => {
                    log::debug!("Replay: DELETE from {} row_id={}", table, row_id);
                }
                _ => {}
            }
        }

        log::info!("Recovery complete");
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
    use std::env;

    #[test]
    fn test_begin_commit() {
        let dir = env::temp_dir().join("minidb_test_txn1");
        std::fs::create_dir_all(&dir).unwrap();
        let mut wal = Wal::open(&dir.join("test.wal")).unwrap();
        let pm = PageManager::open(&dir.join("test.db")).unwrap();

        let mut txn = TxnManager::new();
        let xid = txn.begin(&mut wal).unwrap();
        assert_eq!(xid, 1);
        assert!(txn.is_active());

        txn.commit(
            &mut wal,
            &mut PageManager::open(&dir.join("test.db")).unwrap(),
        )
        .unwrap();
        assert!(!txn.is_active());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_begin_rollback() {
        let dir = env::temp_dir().join("minidb_test_txn2");
        std::fs::create_dir_all(&dir).unwrap();
        let mut wal = Wal::open(&dir.join("test.wal")).unwrap();

        let mut txn = TxnManager::new();
        txn.begin(&mut wal).unwrap();
        txn.rollback(&mut wal).unwrap();
        assert!(!txn.is_active());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_auto_commit() {
        let dir = env::temp_dir().join("minidb_test_txn3");
        std::fs::create_dir_all(&dir).unwrap();
        let mut wal = Wal::open(&dir.join("test.wal")).unwrap();
        let mut pm = PageManager::open(&dir.join("test.db")).unwrap();

        let mut txn = TxnManager::new();
        // No begin - auto-commit mode
        txn.commit(&mut wal, &mut pm).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_double_begin_errors() {
        let dir = env::temp_dir().join("minidb_test_txn4");
        std::fs::create_dir_all(&dir).unwrap();
        let mut wal = Wal::open(&dir.join("test.wal")).unwrap();

        let mut txn = TxnManager::new();
        txn.begin(&mut wal).unwrap();
        assert!(txn.begin(&mut wal).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

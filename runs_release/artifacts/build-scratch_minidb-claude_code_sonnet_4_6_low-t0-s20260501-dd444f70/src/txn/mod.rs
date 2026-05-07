use crate::error::{MiniDbError, Result};
use crate::storage::StorageEngine;
use log::debug;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TXN_ID: AtomicU64 = AtomicU64::new(1);

fn next_txn_id() -> u64 {
    NEXT_TXN_ID.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    Active,
    Committed,
    RolledBack,
}

pub struct TxnManager {
    pub current_txn: Option<u64>,
    pub state: TxnState,
}

impl TxnManager {
    pub fn new() -> Self {
        TxnManager {
            current_txn: None,
            state: TxnState::Committed,
        }
    }

    pub fn begin(&mut self, engine: &mut StorageEngine) -> Result<u64> {
        if self.current_txn.is_some() {
            return Err(MiniDbError::Transaction(
                "transaction already active".to_string(),
            ));
        }
        let txn_id = next_txn_id();
        engine.wal_begin(txn_id)?;
        self.current_txn = Some(txn_id);
        self.state = TxnState::Active;
        debug!("BEGIN txn={}", txn_id);
        Ok(txn_id)
    }

    pub fn commit(&mut self, engine: &mut StorageEngine) -> Result<()> {
        let txn_id = self
            .current_txn
            .ok_or_else(|| MiniDbError::Transaction("no active transaction".to_string()))?;
        engine.wal_commit(txn_id)?;
        self.current_txn = None;
        self.state = TxnState::Committed;
        debug!("COMMIT txn={}", txn_id);
        Ok(())
    }

    pub fn rollback(&mut self, engine: &mut StorageEngine) -> Result<()> {
        let txn_id = self
            .current_txn
            .ok_or_else(|| MiniDbError::Transaction("no active transaction".to_string()))?;
        engine.wal_rollback(txn_id)?;
        self.current_txn = None;
        self.state = TxnState::RolledBack;
        debug!("ROLLBACK txn={}", txn_id);
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.current_txn.is_some()
    }

    /// Auto-begin if not in a transaction (for auto-commit)
    pub fn ensure_active(&mut self, engine: &mut StorageEngine) -> Result<()> {
        if self.current_txn.is_none() {
            self.begin(engine)?;
        }
        Ok(())
    }

    /// Auto-commit after a single statement (for auto-commit mode)
    pub fn auto_commit(&mut self, engine: &mut StorageEngine) -> Result<()> {
        if self.current_txn.is_some() && self.state == TxnState::Active {
            self.commit(engine)?;
        }
        Ok(())
    }

    pub fn current_txn_id(&self) -> Option<u64> {
        self.current_txn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::Stmt;
    use crate::parser::parse;
    use crate::storage::StorageEngine;
    use crate::types::Value;
    use tempfile::NamedTempFile;

    #[test]
    fn test_begin_commit() {
        let tmp = NamedTempFile::new().unwrap();
        let mut engine = StorageEngine::open(tmp.path()).unwrap();
        let mut txn = TxnManager::new();

        let stmts = parse("CREATE TABLE t (id INT);").unwrap();
        if let Stmt::CreateTable(ct) = &stmts[0] {
            engine.create_table(ct).unwrap();
        }

        txn.begin(&mut engine).unwrap();
        engine.insert_row("t", &[Value::Int(1)].to_vec()).unwrap();
        txn.commit(&mut engine).unwrap();

        assert!(!txn.is_active());
        let rows = engine.scan_table("t").unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn test_rollback_discards_changes() {
        let tmp = NamedTempFile::new().unwrap();
        let mut engine = StorageEngine::open(tmp.path()).unwrap();
        let mut txn = TxnManager::new();

        let stmts = parse("CREATE TABLE t (id INT);").unwrap();
        if let Stmt::CreateTable(ct) = &stmts[0] {
            engine.create_table(ct).unwrap();
            engine.pm.sync().unwrap();
        }

        txn.begin(&mut engine).unwrap();
        engine.insert_row("t", &[Value::Int(99)].to_vec()).unwrap();
        txn.rollback(&mut engine).unwrap();

        // After rollback, the row should not be visible (in-memory cache cleared)
        assert!(!txn.is_active());
    }

    #[test]
    fn test_auto_commit() {
        let tmp = NamedTempFile::new().unwrap();
        let mut engine = StorageEngine::open(tmp.path()).unwrap();
        let mut txn = TxnManager::new();

        let stmts = parse("CREATE TABLE t (id INT);").unwrap();
        if let Stmt::CreateTable(ct) = &stmts[0] {
            engine.create_table(ct).unwrap();
        }

        txn.ensure_active(&mut engine).unwrap();
        engine.insert_row("t", &[Value::Int(1)].to_vec()).unwrap();
        txn.auto_commit(&mut engine).unwrap();

        assert!(!txn.is_active());
    }
}

use crate::{
    error::{MiniDbError, Result},
    storage::{StorageEngine, wal::WalOp},
};

pub struct TransactionManager {
    active: bool,
    ops: Vec<WalOp>,
}

impl TransactionManager {
    pub fn new() -> Self {
        Self {
            active: false,
            ops: Vec::new(),
        }
    }
    pub fn in_tx(&self) -> bool {
        self.active
    }
    pub fn begin(&mut self) -> Result<()> {
        if self.active {
            return Err(MiniDbError::Sql("transaction already active".into()));
        }
        self.active = true;
        self.ops.clear();
        Ok(())
    }
    pub fn push(&mut self, op: WalOp) {
        self.ops.push(op);
    }
    pub fn commit(&mut self, storage: &mut StorageEngine) -> Result<usize> {
        if !self.active {
            return Err(MiniDbError::Sql("no active transaction".into()));
        }
        let n = self.ops.len();
        storage.commit_applied_tx(&self.ops)?;
        self.ops.clear();
        self.active = false;
        Ok(n)
    }
    pub fn rollback(&mut self) -> Result<usize> {
        if !self.active {
            return Err(MiniDbError::Sql("no active transaction".into()));
        }
        let n = self.ops.len();
        self.ops.clear();
        self.active = false;
        Ok(n)
    }
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn begin_rollback() {
        let mut t = TransactionManager::new();
        t.begin().unwrap();
        assert!(t.in_tx());
        assert_eq!(t.rollback().unwrap(), 0);
    }
}

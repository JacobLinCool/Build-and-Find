use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone)]
pub struct TransactionManager {
    next_tx: u64,
    active_tx: Option<u64>,
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self {
            next_tx: 1,
            active_tx: None,
        }
    }
}

impl TransactionManager {
    pub fn begin(&mut self) -> Result<u64> {
        if self.active_tx.is_some() {
            return Err(MiniDbError::Sql("transaction already active".into()));
        }
        let tx = self.next_tx;
        self.next_tx += 1;
        self.active_tx = Some(tx);
        Ok(tx)
    }

    pub fn commit(&mut self) -> Result<u64> {
        let tx = self
            .active_tx
            .take()
            .ok_or_else(|| MiniDbError::Sql("no active transaction".into()))?;
        Ok(tx)
    }

    pub fn rollback(&mut self) -> Result<u64> {
        let tx = self
            .active_tx
            .take()
            .ok_or_else(|| MiniDbError::Sql("no active transaction".into()))?;
        Ok(tx)
    }

    pub fn active(&self) -> Option<u64> {
        self.active_tx
    }

    pub fn seed_after(&mut self, tx: u64) {
        self.next_tx = self.next_tx.max(tx + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prevents_nested_transactions() {
        let mut tm = TransactionManager::default();
        tm.begin().unwrap();
        assert!(tm.begin().is_err());
    }
}

use crate::error::MiniDbError;

#[derive(Debug, Default)]
pub struct TxnManager {
    pub active: bool,
}

impl TxnManager {
    pub fn begin(&mut self) -> Result<(), MiniDbError> {
        if self.active {
            return Err(MiniDbError::txn("transaction already active"));
        }
        self.active = true;
        Ok(())
    }
    pub fn commit(&mut self) -> Result<(), MiniDbError> {
        if !self.active {
            return Err(MiniDbError::txn("no active transaction"));
        }
        self.active = false;
        Ok(())
    }
    pub fn rollback(&mut self) -> Result<(), MiniDbError> {
        if !self.active {
            return Err(MiniDbError::txn("no active transaction"));
        }
        self.active = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn txn_lifecycle() {
        let mut t = TxnManager::default();
        t.begin().unwrap();
        assert!(t.commit().is_ok());
    }
}

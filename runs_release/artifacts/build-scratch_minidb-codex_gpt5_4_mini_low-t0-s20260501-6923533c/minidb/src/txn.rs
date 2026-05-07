use crate::{
    catalog::Catalog,
    error::Result,
    storage::{Storage, wal::WalRecord},
};

#[derive(Clone)]
pub struct TransactionManager {
    pub explicit: bool,
}

impl TransactionManager {
    pub fn new() -> Self {
        Self { explicit: false }
    }
    pub fn begin(&mut self) {
        self.explicit = true;
    }
    pub fn commit(&mut self, storage: &Storage, catalog: &Catalog) -> Result<()> {
        storage.wal_append(&WalRecord::Snapshot(catalog.clone()))?;
        storage.persist(catalog)?;
        storage.wal_clear()?;
        self.explicit = false;
        Ok(())
    }
    pub fn rollback(&mut self) {
        self.explicit = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tx_state() {
        assert!(!TransactionManager::new().explicit);
    }
}

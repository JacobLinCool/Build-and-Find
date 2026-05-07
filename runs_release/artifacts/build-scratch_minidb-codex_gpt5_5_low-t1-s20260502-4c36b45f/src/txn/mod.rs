use crate::{
    error::{MiniDbError, Result},
    exec::{Executor, QueryResult},
    parser::ast::Statement,
    storage::{Storage, wal::WalRecord},
};

pub struct TransactionManager {
    active: Option<u64>,
    next_tx: u64,
    snapshot: Option<crate::storage::DatabaseImage>,
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self {
            active: None,
            next_tx: 1,
            snapshot: None,
        }
    }
}

impl TransactionManager {
    pub fn begin(&mut self, storage: &mut Storage) -> Result<QueryResult> {
        if self.active.is_some() {
            return Err(MiniDbError::Txn("transaction already active".into()));
        }
        let tx = self.next_tx;
        self.next_tx += 1;
        self.snapshot = Some(storage.image.clone());
        storage.append_wal(&WalRecord::Begin(tx))?;
        self.active = Some(tx);
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected: Some(0),
            message: "ok begin".into(),
        })
    }

    pub fn commit(&mut self, storage: &mut Storage) -> Result<QueryResult> {
        let tx = self
            .active
            .take()
            .ok_or_else(|| MiniDbError::Txn("no transaction active".into()))?;
        storage.append_wal(&WalRecord::Commit(tx))?;
        storage.flush()?;
        storage.clear_wal()?;
        self.snapshot = None;
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected: Some(0),
            message: "ok commit".into(),
        })
    }

    pub fn rollback(&mut self, storage: &mut Storage) -> Result<QueryResult> {
        let tx = self
            .active
            .take()
            .ok_or_else(|| MiniDbError::Txn("no transaction active".into()))?;
        storage.append_wal(&WalRecord::Rollback(tx))?;
        if let Some(snapshot) = self.snapshot.take() {
            storage.image = snapshot;
        }
        storage.clear_wal()?;
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected: Some(0),
            message: "ok rollback".into(),
        })
    }

    pub fn execute(&mut self, storage: &mut Storage, stmt: &Statement) -> Result<QueryResult> {
        match stmt {
            Statement::Begin => self.begin(storage),
            Statement::Commit => self.commit(storage),
            Statement::Rollback => self.rollback(storage),
            _ if self.active.is_some() => {
                let tx = self.active.map_or(0, |tx| tx);
                let before = storage.image.clone();
                let res = Executor { storage }.execute(stmt);
                match res {
                    Ok(q) => {
                        storage.append_wal(&WalRecord::Statement {
                            tx,
                            statement: stmt.clone(),
                        })?;
                        Ok(q)
                    }
                    Err(e) => {
                        storage.image = before;
                        Err(e)
                    }
                }
            }
            _ => {
                self.begin(storage)?;
                let res = Executor { storage }.execute(stmt);
                match res {
                    Ok(q) => {
                        let tx = self.active.map_or(0, |tx| tx);
                        storage.append_wal(&WalRecord::Statement {
                            tx,
                            statement: stmt.clone(),
                        })?;
                        self.commit(storage)?;
                        Ok(q)
                    }
                    Err(e) => {
                        let _ = self.rollback(storage);
                        Err(e)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_has_no_active_tx() {
        assert!(TransactionManager::default().active.is_none());
    }
}

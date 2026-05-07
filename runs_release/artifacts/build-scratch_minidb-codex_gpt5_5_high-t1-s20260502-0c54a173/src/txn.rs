use crate::error::{MiniDbError, Result};
use crate::exec::{QueryOutput, execute_statement};
use crate::parser::ast::Statement;
use crate::storage::wal::{WalRecord, append};
use crate::storage::{DatabaseState, DatabaseStore};
use std::path::Path;

pub struct TransactionManager {
    store: DatabaseStore,
    state: DatabaseState,
    active: Option<ActiveTxn>,
    next_tx: u64,
}

struct ActiveTxn {
    id: u64,
    working: DatabaseState,
}

impl TransactionManager {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let (store, state) = DatabaseStore::open(path)?;
        Ok(Self {
            store,
            state,
            active: None,
            next_tx: 1,
        })
    }

    pub fn init(path: impl AsRef<Path>) -> Result<()> {
        DatabaseStore::init(path)
    }

    pub fn state(&self) -> &DatabaseState {
        match self.active.as_ref() {
            Some(tx) => &tx.working,
            None => &self.state,
        }
    }

    pub fn execute(&mut self, statement: Statement) -> Result<QueryOutput> {
        match statement {
            Statement::Begin => {
                self.begin()?;
                Ok(QueryOutput::ok())
            }
            Statement::Commit => {
                self.commit()?;
                Ok(QueryOutput::ok())
            }
            Statement::Rollback => {
                self.rollback()?;
                Ok(QueryOutput::ok())
            }
            Statement::Select(select) => {
                if let Some(active) = self.active.as_mut() {
                    execute_statement(&mut active.working, Statement::Select(select))
                } else {
                    execute_statement(&mut self.state, Statement::Select(select))
                }
            }
            other => {
                if self.active.is_some() {
                    self.execute_in_active(other)
                } else {
                    self.execute_auto(other)
                }
            }
        }
    }

    pub fn begin(&mut self) -> Result<()> {
        if self.active.is_some() {
            return Err(MiniDbError::sql("transaction already active"));
        }
        let id = self.allocate_tx();
        append(self.store.wal_path(), &WalRecord::Begin { tx: id })?;
        self.active = Some(ActiveTxn {
            id,
            working: self.state.clone(),
        });
        Ok(())
    }

    pub fn commit(&mut self) -> Result<()> {
        let active = self
            .active
            .take()
            .ok_or_else(|| MiniDbError::sql("no active transaction"))?;
        append(self.store.wal_path(), &WalRecord::Commit { tx: active.id })?;
        self.state = active.working;
        Ok(())
    }

    pub fn rollback(&mut self) -> Result<()> {
        let active = self
            .active
            .take()
            .ok_or_else(|| MiniDbError::sql("no active transaction"))?;
        append(
            self.store.wal_path(),
            &WalRecord::Rollback { tx: active.id },
        )?;
        Ok(())
    }

    fn execute_in_active(&mut self, statement: Statement) -> Result<QueryOutput> {
        let active = self
            .active
            .as_mut()
            .ok_or_else(|| MiniDbError::sql("no active transaction"))?;
        let mut candidate = active.working.clone();
        let output = execute_statement(&mut candidate, statement.clone())?;
        append(
            self.store.wal_path(),
            &WalRecord::Statement {
                tx: active.id,
                statement,
            },
        )?;
        active.working = candidate;
        Ok(output)
    }

    fn execute_auto(&mut self, statement: Statement) -> Result<QueryOutput> {
        let id = self.allocate_tx();
        let mut candidate = self.state.clone();
        let output = execute_statement(&mut candidate, statement.clone())?;
        append(self.store.wal_path(), &WalRecord::Begin { tx: id })?;
        append(
            self.store.wal_path(),
            &WalRecord::Statement { tx: id, statement },
        )?;
        append(self.store.wal_path(), &WalRecord::Commit { tx: id })?;
        self.state = candidate;
        Ok(output)
    }

    fn allocate_tx(&mut self) -> u64 {
        let id = self.next_tx;
        self.next_tx = self.next_tx.saturating_add(1);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_sql;

    #[test]
    fn rollback_discards_working_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut tm = TransactionManager::open(&path).unwrap();
        for stmt in parse_sql("create table t (id int); begin; insert into t values (1); rollback;")
            .unwrap()
        {
            tm.execute(stmt).unwrap();
        }
        let out = tm
            .execute(parse_sql("select * from t;").unwrap().remove(0))
            .unwrap();
        assert!(out.rows.is_empty());
    }
}

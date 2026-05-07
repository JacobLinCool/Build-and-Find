use crate::error::{MiniDbError, Result};
use crate::exec::{apply_statement_direct, ExecutionResult};
use crate::parser::ast::Statement;
use crate::storage::{wal::WalRecord, DatabaseState, StorageEngine};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Transaction {
    pub txid: u64,
    pub state: DatabaseState,
    pub statements: Vec<Statement>,
}

#[derive(Debug)]
pub struct Database {
    storage: StorageEngine,
    active: Option<Transaction>,
    next_txid: u64,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let storage = StorageEngine::open(path)?;
        let next_txid =
            storage
                .state
                .applied_txid
                .max(crate::storage::wal::Wal::max_txid_from_path(
                    storage.wal_path(),
                )?)
                + 1;
        Ok(Self {
            storage,
            active: None,
            next_txid,
        })
    }

    pub fn init(path: impl AsRef<Path>) -> Result<()> {
        StorageEngine::init(path)
    }

    pub fn begin(&mut self) -> Result<()> {
        if self.active.is_some() {
            return Err(MiniDbError::runtime("transaction already active"));
        }
        let txid = self.allocate_txid();
        self.active = Some(Transaction {
            txid,
            state: self.storage.state.clone(),
            statements: Vec::new(),
        });
        Ok(())
    }

    pub fn rollback(&mut self) -> Result<ExecutionResult> {
        let tx = self
            .active
            .take()
            .ok_or_else(|| MiniDbError::runtime("no active transaction"))?;
        self.storage
            .wal_mut()
            .append_record(&WalRecord::Rollback { txid: tx.txid })?;
        Ok(ExecutionResult::Message("transaction rolled back".into()))
    }

    pub fn commit(&mut self) -> Result<ExecutionResult> {
        let tx = self
            .active
            .take()
            .ok_or_else(|| MiniDbError::runtime("no active transaction"))?;
        self.storage
            .wal_mut()
            .append_record(&WalRecord::Begin { txid: tx.txid })?;
        for statement in &tx.statements {
            self.storage
                .wal_mut()
                .append_record(&WalRecord::Statement {
                    txid: tx.txid,
                    statement: statement.clone(),
                })?;
        }
        self.storage
            .wal_mut()
            .append_record(&WalRecord::Commit { txid: tx.txid })?;
        self.storage.state = tx.state;
        self.storage.state.applied_txid = tx.txid;
        self.storage.save()?;
        Ok(ExecutionResult::Message("transaction committed".into()))
    }

    pub fn execute_statement(&mut self, statement: Statement) -> Result<ExecutionResult> {
        match statement {
            Statement::Begin => {
                self.begin()?;
                Ok(ExecutionResult::Message("transaction started".into()))
            }
            Statement::Commit => self.commit(),
            Statement::Rollback => self.rollback(),
            statement => {
                if self.active.is_some() {
                    self.execute_in_active(statement)
                } else {
                    self.execute_autocommit(statement)
                }
            }
        }
    }

    fn execute_in_active(&mut self, statement: Statement) -> Result<ExecutionResult> {
        let tx = self
            .active
            .as_mut()
            .ok_or_else(|| MiniDbError::runtime("no active transaction"))?;
        let mut next_state = tx.state.clone();
        let result = apply_statement_direct(&mut next_state, &statement)?;
        tx.state = next_state;
        if statement_mutates(&statement) {
            tx.statements.push(statement);
        }
        Ok(result)
    }

    fn execute_autocommit(&mut self, statement: Statement) -> Result<ExecutionResult> {
        let txid = self.allocate_txid();
        let mut state = self.storage.state.clone();
        let result = apply_statement_direct(&mut state, &statement)?;
        self.storage
            .wal_mut()
            .append_record(&WalRecord::Begin { txid })?;
        if statement_mutates(&statement) {
            self.storage
                .wal_mut()
                .append_record(&WalRecord::Statement {
                    txid,
                    statement: statement.clone(),
                })?;
        }
        self.storage
            .wal_mut()
            .append_record(&WalRecord::Commit { txid })?;
        self.storage.state = state;
        self.storage.state.applied_txid = txid;
        self.storage.save()?;
        Ok(result)
    }

    fn allocate_txid(&mut self) -> u64 {
        let txid = self.next_txid;
        self.next_txid += 1;
        txid
    }

    pub fn state(&self) -> &DatabaseState {
        &self.storage.state
    }
}

fn statement_mutates(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::CreateTable(_)
            | Statement::DropTable { .. }
            | Statement::CreateIndex { .. }
            | Statement::DropIndex { .. }
            | Statement::Insert(_)
            | Statement::Update(_)
            | Statement::Delete(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_sql;
    use tempfile::tempdir;

    #[test]
    fn auto_commit_persists_rows() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("db");
        Database::init(&path).unwrap();
        let mut db = Database::open(&path).unwrap();
        for statement in
            parse_sql("CREATE TABLE t (id INT PRIMARY KEY); INSERT INTO t VALUES (1);").unwrap()
        {
            db.execute_statement(statement).unwrap();
        }
        assert_eq!(db.state().tables["t"].rows.len(), 1);
    }
}

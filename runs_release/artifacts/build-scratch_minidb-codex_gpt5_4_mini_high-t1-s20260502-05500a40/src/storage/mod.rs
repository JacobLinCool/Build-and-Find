pub mod btree;
pub mod heap;
pub mod page;
pub mod wal;

use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast::Statement;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageId(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseState {
    pub catalog: Catalog,
    pub tables: BTreeMap<String, heap::HeapTable>,
    pub applied_txid: u64,
}

impl Default for DatabaseState {
    fn default() -> Self {
        Self {
            catalog: Catalog::default(),
            tables: BTreeMap::new(),
            applied_txid: 0,
        }
    }
}

#[derive(Debug)]
pub struct StorageEngine {
    db_path: PathBuf,
    pub(crate) wal_path: PathBuf,
    pub state: DatabaseState,
    pub(crate) wal: wal::Wal,
}

impl StorageEngine {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        let wal_path = wal::wal_path(&db_path);
        let wal = wal::Wal::open(&wal_path)?;
        let mut state = if db_path.exists() {
            Self::load_state(&db_path)?
        } else {
            DatabaseState::default()
        };
        wal.replay(&mut state)?;
        Ok(Self {
            db_path,
            wal_path,
            state,
            wal,
        })
    }

    pub fn init(path: impl AsRef<Path>) -> Result<()> {
        let db_path = path.as_ref();
        let wal_path = wal::wal_path(db_path);
        Self::write_state(db_path, &DatabaseState::default())?;
        fs::File::create(wal_path)?;
        Ok(())
    }

    pub fn save(&mut self) -> Result<()> {
        Self::write_state(&self.db_path, &self.state)
    }

    pub fn wal_mut(&mut self) -> &mut wal::Wal {
        &mut self.wal
    }

    pub fn wal_path(&self) -> &Path {
        &self.wal_path
    }

    pub fn append_statement(&mut self, txid: u64, stmt: &Statement, committed: bool) -> Result<()> {
        self.wal.append_record(&wal::WalRecord::Statement {
            txid,
            statement: stmt.clone(),
        })?;
        if committed {
            self.wal.append_record(&wal::WalRecord::Commit { txid })?;
            self.state.applied_txid = txid;
            self.save()?;
        }
        Ok(())
    }

    pub fn append_begin(&mut self, txid: u64) -> Result<()> {
        self.wal.append_record(&wal::WalRecord::Begin { txid })
    }

    pub fn append_rollback(&mut self, txid: u64) -> Result<()> {
        self.wal.append_record(&wal::WalRecord::Rollback { txid })
    }

    fn load_state(path: &Path) -> Result<DatabaseState> {
        let bytes = fs::read(path)?;
        if bytes.is_empty() {
            return Ok(DatabaseState::default());
        }
        let state: DatabaseState =
            bincode::deserialize(&bytes).map_err(|err| MiniDbError::storage(err.to_string()))?;
        Ok(state)
    }

    fn write_state(path: &Path, state: &DatabaseState) -> Result<()> {
        let tmp = path.with_extension("tmp");
        let bytes =
            bincode::serialize(state).map_err(|err| MiniDbError::storage(err.to_string()))?;
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, path)?;
        Ok(())
    }
}

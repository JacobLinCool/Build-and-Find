pub mod btree;
pub mod heap;
pub mod wal;

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::Catalog;
use crate::error::MiniDbError;
use crate::types::Row;

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageId(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DbSnapshot {
    pub catalog: Catalog,
    pub rows: std::collections::BTreeMap<String, Vec<Row>>,
    pub indexes: std::collections::BTreeMap<String, btree::IndexData>,
}

pub struct Storage {
    path: PathBuf,
    wal_path: PathBuf,
    snapshot: DbSnapshot,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MiniDbError> {
        let path = path.as_ref().to_path_buf();
        let wal_path = path.with_extension("wal");
        let mut snapshot = if path.exists() {
            let mut buf = Vec::new();
            File::open(&path)?.read_to_end(&mut buf)?;
            if buf.is_empty() {
                DbSnapshot::default()
            } else {
                serde_json::from_slice(&buf).map_err(|e| MiniDbError::storage(e.to_string()))?
            }
        } else {
            DbSnapshot::default()
        };
        wal::replay(&wal_path, &mut snapshot)?;
        Ok(Self {
            path,
            wal_path,
            snapshot,
        })
    }
    pub fn init(path: impl AsRef<Path>) -> Result<(), MiniDbError> {
        let path = path.as_ref();
        let wal = path.with_extension("wal");
        File::create(path)?;
        File::create(wal)?;
        Ok(())
    }
    pub fn snapshot(&self) -> &DbSnapshot {
        &self.snapshot
    }
    pub fn snapshot_mut(&mut self) -> &mut DbSnapshot {
        &mut self.snapshot
    }
    pub fn persist(&self) -> Result<(), MiniDbError> {
        let bytes =
            serde_json::to_vec(&self.snapshot).map_err(|e| MiniDbError::storage(e.to_string()))?;
        let mut f = File::create(&self.path)?;
        f.write_all(&bytes)?;
        f.flush()?;
        Ok(())
    }
    pub fn wal_append(&self, rec: &wal::WalRecord) -> Result<(), MiniDbError> {
        wal::append(&self.wal_path, rec)
    }
    pub fn wal_flush(&self) -> Result<(), MiniDbError> {
        Ok(())
    }
}

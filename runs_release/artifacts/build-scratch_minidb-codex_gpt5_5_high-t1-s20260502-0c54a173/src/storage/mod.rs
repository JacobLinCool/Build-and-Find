pub mod btree;
pub mod heap;
pub mod page;
pub mod wal;

use crate::catalog::Catalog;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PageId(pub u64);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatabaseState {
    pub catalog: Catalog,
    pub tables: BTreeMap<String, heap::TableData>,
    pub indexes: BTreeMap<String, btree::BTreeIndex>,
}

pub struct DatabaseStore {
    db_path: PathBuf,
    wal_path: PathBuf,
}

impl DatabaseStore {
    pub fn open(path: impl AsRef<Path>) -> Result<(Self, DatabaseState)> {
        let db_path = path.as_ref().to_path_buf();
        page::ensure_database_file(&db_path)?;
        let wal_path = db_path.with_extension("wal");
        let state = wal::recover(&wal_path)?;
        Ok((Self { db_path, wal_path }, state))
    }

    pub fn init(path: impl AsRef<Path>) -> Result<()> {
        let db_path = path.as_ref();
        page::initialize_database_file(db_path)?;
        let wal_path = db_path.with_extension("wal");
        if wal_path.exists() {
            std::fs::remove_file(wal_path)?;
        }
        Ok(())
    }

    pub fn wal_path(&self) -> &Path {
        &self.wal_path
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_size_is_fixed() {
        assert_eq!(PAGE_SIZE, 4096);
        let page = PageId(7);
        assert_eq!(page.0, 7);
    }
}

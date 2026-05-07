pub mod btree;
pub mod heap;
pub mod wal;

use crate::{catalog::Catalog, error::Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const PAGE_SIZE: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PageId(pub u64);

#[derive(Clone)]
pub struct Storage {
    path: PathBuf,
    wal_path: PathBuf,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<(Self, Catalog)> {
        let path = path.as_ref().to_path_buf();
        let wal_path = path.with_extension("wal");
        let mut catalog = if path.exists() {
            serde_json::from_str(&fs::read_to_string(&path)?).unwrap_or_default()
        } else {
            Catalog::default()
        };
        catalog = wal::replay(&wal_path, catalog)?;
        Ok((Self { path, wal_path }, catalog))
    }
    pub fn init(path: impl AsRef<Path>) -> Result<()> {
        let p = path.as_ref();
        fs::write(p, serde_json::to_string(&Catalog::default())?)?;
        fs::write(p.with_extension("wal"), "")?;
        Ok(())
    }
    pub fn persist(&self, catalog: &Catalog) -> Result<()> {
        fs::write(&self.path, serde_json::to_string_pretty(catalog)?)?;
        Ok(())
    }
    pub fn wal_append(&self, record: &wal::WalRecord) -> Result<()> {
        wal::append(&self.wal_path, record)
    }
    pub fn wal_clear(&self) -> Result<()> {
        fs::write(&self.wal_path, "")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_size_constant() {
        assert_eq!(PAGE_SIZE, 4096);
    }
}

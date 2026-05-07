pub mod btree;
pub mod heap;
pub mod page;
pub mod wal;

use std::path::Path;

use crate::error::Result;
use page::PageManager;
use wal::Wal;

pub struct Storage {
    pub pm: PageManager,
    pub wal: Wal,
}

impl Storage {
    pub fn open(db_path: &Path) -> Result<Self> {
        let wal_path = db_path.with_extension("wal");
        let pm = PageManager::open(db_path)?;
        let wal = Wal::open(&wal_path)?;
        Ok(Self { pm, wal })
    }

    pub fn flush(&mut self) -> Result<()> {
        self.pm.flush()?;
        self.wal.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_storage_open() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let storage = Storage::open(&path).unwrap();
        assert_eq!(storage.pm.page_count(), 0);
    }
}

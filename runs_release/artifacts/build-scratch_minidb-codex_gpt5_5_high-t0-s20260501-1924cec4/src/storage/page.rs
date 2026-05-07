use crate::error::{MiniDbError, Result};
use crate::storage::{PageId, PAGE_SIZE};
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub struct PageManager {
    path: PathBuf,
}

impl PageManager {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn read_page(&self, id: PageId) -> Result<[u8; PAGE_SIZE]> {
        let mut file = OpenOptions::new().read(true).open(&self.path)?;
        file.seek(SeekFrom::Start(id.0 * PAGE_SIZE as u64))?;
        let mut page = [0u8; PAGE_SIZE];
        file.read_exact(&mut page)
            .map_err(|e| MiniDbError::Storage(format!("could not read page {}: {e}", id.0)))?;
        Ok(page)
    }

    pub fn write_page(&self, id: PageId, page: &[u8; PAGE_SIZE]) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .open(&self.path)?;
        file.seek(SeekFrom::Start(id.0 * PAGE_SIZE as u64))?;
        file.write_all(page)?;
        file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_and_reads_fixed_size_pages() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("pages.bin");
        let mgr = PageManager::new(&path);
        let mut page = [0u8; PAGE_SIZE];
        page[0] = 42;
        mgr.write_page(PageId(0), &page).unwrap();
        assert_eq!(mgr.read_page(PageId(0)).unwrap()[0], 42);
    }
}

use crate::error::{MiniDbError, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PageId(pub u64);

impl PageId {
    pub fn to_offset(self) -> u64 {
        self.0 * PAGE_SIZE as u64
    }
}

impl std::fmt::Display for PageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Page({})", self.0)
    }
}

pub type Page = [u8; PAGE_SIZE];

pub struct PageManager {
    file: File,
    page_count: u64,
}

impl PageManager {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let meta = file.metadata()?;
        let page_count = meta.len() / PAGE_SIZE as u64;
        Ok(Self { file, page_count })
    }

    pub fn create(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        Ok(Self {
            file,
            page_count: 0,
        })
    }

    pub fn page_count(&self) -> u64 {
        self.page_count
    }

    pub fn alloc_page(&mut self) -> Result<PageId> {
        let pid = PageId(self.page_count);
        let buf = [0u8; PAGE_SIZE];
        self.file.seek(SeekFrom::Start(pid.to_offset()))?;
        self.file.write_all(&buf)?;
        self.page_count += 1;
        Ok(pid)
    }

    pub fn read_page(&mut self, pid: PageId) -> Result<Box<Page>> {
        if pid.0 >= self.page_count {
            return Err(MiniDbError::Storage(format!(
                "page {} out of range (count={})",
                pid.0, self.page_count
            )));
        }
        let mut buf = Box::new([0u8; PAGE_SIZE]);
        self.file.seek(SeekFrom::Start(pid.to_offset()))?;
        self.file.read_exact(&mut *buf)?;
        Ok(buf)
    }

    pub fn write_page(&mut self, pid: PageId, page: &Page) -> Result<()> {
        if pid.0 >= self.page_count {
            return Err(MiniDbError::Storage(format!(
                "page {} out of range (count={})",
                pid.0, self.page_count
            )));
        }
        self.file.seek(SeekFrom::Start(pid.to_offset()))?;
        self.file.write_all(page)?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.file.flush()?;
        self.file.sync_data()?;
        Ok(())
    }

    pub fn reopen(&mut self, path: &Path) -> Result<()> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let meta = file.metadata()?;
        self.page_count = meta.len() / PAGE_SIZE as u64;
        self.file = file;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_page_manager_basic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::create(&path).unwrap();
        assert_eq!(pm.page_count(), 0);

        let pid = pm.alloc_page().unwrap();
        assert_eq!(pid, PageId(0));
        assert_eq!(pm.page_count(), 1);

        let mut page = [0u8; PAGE_SIZE];
        page[0] = 42;
        pm.write_page(pid, &page).unwrap();

        let read = pm.read_page(pid).unwrap();
        assert_eq!(read[0], 42);
        assert_eq!(read[1], 0);

        pm.flush().unwrap();
    }

    #[test]
    fn test_page_manager_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let mut pm = PageManager::create(&path).unwrap();
            let pid = pm.alloc_page().unwrap();
            let mut page = [0u8; PAGE_SIZE];
            page[0] = 99;
            pm.write_page(pid, &page).unwrap();
            pm.flush().unwrap();
        }
        {
            let mut pm = PageManager::open(&path).unwrap();
            assert_eq!(pm.page_count(), 1);
            let read = pm.read_page(PageId(0)).unwrap();
            assert_eq!(read[0], 99);
        }
    }
}

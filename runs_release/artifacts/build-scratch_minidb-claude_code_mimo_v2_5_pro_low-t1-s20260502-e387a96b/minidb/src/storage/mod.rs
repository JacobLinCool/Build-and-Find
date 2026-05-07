pub mod btree;
pub mod heap;
pub mod page;
pub mod wal;

use crate::error::Result;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageId(pub u64);

impl PageId {
    pub fn to_offset(self) -> u64 {
        self.0 * PAGE_SIZE as u64
    }
}

pub struct Pager {
    file: Arc<Mutex<File>>,
    page_count: u64,
}

impl Pager {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let meta = file.metadata()?;
        let page_count = meta.len() / PAGE_SIZE as u64;
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
            page_count,
        })
    }

    pub fn page_count(&self) -> u64 {
        self.page_count
    }

    pub fn alloc_page(&mut self) -> PageId {
        let pid = PageId(self.page_count);
        self.page_count += 1;
        pid
    }

    pub fn read_page(&self, pid: PageId) -> Result<[u8; PAGE_SIZE]> {
        let mut buf = [0u8; PAGE_SIZE];
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(pid.to_offset()))?;
        file.read_exact(&mut buf)?;
        Ok(buf)
    }

    pub fn write_page(&self, pid: PageId, data: &[u8; PAGE_SIZE]) -> Result<()> {
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(pid.to_offset()))?;
        file.write_all(data)?;
        file.flush()?;
        Ok(())
    }

    pub fn sync(&self) -> Result<()> {
        let file = self.file.lock().unwrap();
        file.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_pager_alloc_and_rw() {
        let path = std::env::temp_dir().join("minidb_pager_test.db");
        let _ = fs::remove_file(&path);
        {
            let mut pager = Pager::open(&path).unwrap();
            assert_eq!(pager.page_count(), 0);
            let pid = pager.alloc_page();
            assert_eq!(pid, PageId(0));
            let mut data = [0u8; PAGE_SIZE];
            data[0] = 0xAB;
            data[PAGE_SIZE - 1] = 0xCD;
            pager.write_page(pid, &data).unwrap();
            let read = pager.read_page(pid).unwrap();
            assert_eq!(read[0], 0xAB);
            assert_eq!(read[PAGE_SIZE - 1], 0xCD);
        }
        let _ = fs::remove_file(&path);
    }
}

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PageId(pub u64);

impl PageId {
    pub fn to_offset(self) -> u64 {
        self.0 * PAGE_SIZE as u64
    }
}

pub struct PageManager {
    file: File,
    page_count: u64,
}

impl PageManager {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let metadata = file.metadata()?;
        let page_count = metadata.len() / PAGE_SIZE as u64;
        Ok(Self { file, page_count })
    }

    pub fn page_count(&self) -> u64 {
        self.page_count
    }

    pub fn allocate_page(&mut self) -> io::Result<PageId> {
        let pid = PageId(self.page_count);
        self.page_count += 1;
        let page = [0u8; PAGE_SIZE];
        self.write_page(pid, &page)?;
        Ok(pid)
    }

    pub fn read_page(&mut self, pid: PageId) -> io::Result<[u8; PAGE_SIZE]> {
        let mut buf = [0u8; PAGE_SIZE];
        self.file.seek(SeekFrom::Start(pid.to_offset()))?;
        self.file.read_exact(&mut buf)?;
        Ok(buf)
    }

    pub fn write_page(&mut self, pid: PageId, data: &[u8; PAGE_SIZE]) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(pid.to_offset()))?;
        self.file.write_all(data)?;
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.file.flush()?;
        self.file.sync_data()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_allocate_and_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::open(&path).unwrap();
        assert_eq!(pm.page_count(), 0);
        let pid = pm.allocate_page().unwrap();
        assert_eq!(pid, PageId(0));
        assert_eq!(pm.page_count(), 1);
        let mut data = [0u8; PAGE_SIZE];
        data[0] = 42;
        pm.write_page(pid, &data).unwrap();
        let read = pm.read_page(pid).unwrap();
        assert_eq!(read[0], 42);
    }

    #[test]
    fn test_multiple_pages() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::open(&path).unwrap();
        let p0 = pm.allocate_page().unwrap();
        let p1 = pm.allocate_page().unwrap();
        assert_eq!(p0, PageId(0));
        assert_eq!(p1, PageId(1));
        let mut data = [0u8; PAGE_SIZE];
        data[0] = 1;
        pm.write_page(p0, &data).unwrap();
        data[0] = 2;
        pm.write_page(p1, &data).unwrap();
        assert_eq!(pm.read_page(p0).unwrap()[0], 1);
        assert_eq!(pm.read_page(p1).unwrap()[0], 2);
    }
}

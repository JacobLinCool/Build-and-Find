use crate::error::Result;
use crate::storage::{PageId, PAGE_SIZE};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

#[derive(Debug)]
pub struct PageManager {
    file: File,
}

impl PageManager {
    pub fn new(file: File) -> Self {
        Self { file }
    }

    pub fn read_page(&mut self, page_id: PageId) -> Result<[u8; PAGE_SIZE]> {
        let mut buf = [0u8; PAGE_SIZE];
        self.file
            .seek(SeekFrom::Start(page_id.0 * PAGE_SIZE as u64))?;
        let _ = self.file.read(&mut buf)?;
        Ok(buf)
    }

    pub fn write_page(&mut self, page_id: PageId, data: &[u8; PAGE_SIZE]) -> Result<()> {
        self.file
            .seek(SeekFrom::Start(page_id.0 * PAGE_SIZE as u64))?;
        self.file.write_all(data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempfile;

    #[test]
    fn reads_and_writes_pages() {
        let file = tempfile().unwrap();
        let mut pm = PageManager::new(file);
        let mut page = [0u8; PAGE_SIZE];
        page[0] = 7;
        pm.write_page(PageId(0), &page).unwrap();
        let read = pm.read_page(PageId(0)).unwrap();
        assert_eq!(read[0], 7);
    }
}

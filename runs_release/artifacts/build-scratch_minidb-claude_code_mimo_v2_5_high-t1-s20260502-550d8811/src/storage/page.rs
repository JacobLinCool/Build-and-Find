use crate::error::{MiniDbError, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageId(pub u64);

#[derive(Debug, Clone)]
pub struct Page {
    pub id: PageId,
    pub data: [u8; PAGE_SIZE],
    pub dirty: bool,
}

impl Page {
    pub fn new(id: PageId) -> Self {
        Page {
            id,
            data: [0u8; PAGE_SIZE],
            dirty: false,
        }
    }
}

pub struct PageManager {
    file: File,
    path: String,
    num_pages: u64,
}

impl PageManager {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let metadata = file.metadata()?;
        let file_len = metadata.len();
        let num_pages = if file_len == 0 {
            0
        } else {
            (file_len + PAGE_SIZE as u64 - 1) / PAGE_SIZE as u64
        };

        log::info!(
            "Opened page manager: {} pages, {} bytes",
            num_pages,
            file_len
        );

        Ok(PageManager {
            file,
            path: path.to_string_lossy().to_string(),
            num_pages,
        })
    }

    pub fn alloc_page(&mut self) -> Result<PageId> {
        let id = PageId(self.num_pages);
        self.num_pages += 1;
        // Extend file with a zero page
        self.file.seek(SeekFrom::Start(id.0 * PAGE_SIZE as u64))?;
        let zeros = [0u8; PAGE_SIZE];
        self.file.write_all(&zeros)?;
        self.file.flush()?;
        log::debug!("Allocated page {}", id.0);
        Ok(id)
    }

    pub fn read_page(&mut self, id: PageId) -> Result<Page> {
        if id.0 >= self.num_pages {
            return Err(MiniDbError::Storage(format!(
                "page {} does not exist (num_pages={})",
                id.0, self.num_pages
            )));
        }
        let mut page = Page::new(id);
        self.file.seek(SeekFrom::Start(id.0 * PAGE_SIZE as u64))?;
        self.file.read_exact(&mut page.data)?;
        Ok(page)
    }

    pub fn write_page(&mut self, page: &Page) -> Result<()> {
        if page.id.0 >= self.num_pages {
            self.num_pages = page.id.0 + 1;
        }
        self.file
            .seek(SeekFrom::Start(page.id.0 * PAGE_SIZE as u64))?;
        self.file.write_all(&page.data)?;
        self.file.flush()?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.file.flush()?;
        Ok(())
    }

    pub fn num_pages(&self) -> u64 {
        self.num_pages
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_page_alloc_and_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::open(&path).unwrap();

        let id = pm.alloc_page().unwrap();
        assert_eq!(id, PageId(0));

        let mut page = Page::new(id);
        page.data[0..5].copy_from_slice(b"hello");
        page.dirty = true;
        pm.write_page(&page).unwrap();

        let read = pm.read_page(id).unwrap();
        assert_eq!(&read.data[0..5], b"hello");
    }

    #[test]
    fn test_multiple_pages() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::open(&path).unwrap();

        let id1 = pm.alloc_page().unwrap();
        let id2 = pm.alloc_page().unwrap();
        assert_eq!(id1, PageId(0));
        assert_eq!(id2, PageId(1));

        let mut p1 = Page::new(id1);
        p1.data[0] = 42;
        pm.write_page(&p1).unwrap();

        let mut p2 = Page::new(id2);
        p2.data[0] = 99;
        pm.write_page(&p2).unwrap();

        assert_eq!(pm.read_page(id1).unwrap().data[0], 42);
        assert_eq!(pm.read_page(id2).unwrap().data[0], 99);
    }
}

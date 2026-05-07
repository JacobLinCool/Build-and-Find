use crate::error::{MiniDbError, Result};
use crate::storage::page::{Page, PageId, PAGE_SIZE};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Simple page manager: reads/writes pages from a file.
/// Pages are stored sequentially: page N is at byte offset N * PAGE_SIZE.
pub struct Pager {
    file: File,
    page_count: u64,
    cache: HashMap<u64, Page>,
}

impl Pager {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let metadata = file.metadata()?;
        let page_count = metadata.len() / PAGE_SIZE as u64;

        Ok(Pager {
            file,
            page_count,
            cache: HashMap::new(),
        })
    }

    pub fn page_count(&self) -> u64 {
        self.page_count
    }

    /// Allocate a new page and return its ID
    pub fn allocate_page(&mut self) -> Result<PageId> {
        let id = PageId(self.page_count);
        self.page_count += 1;
        // Write zeroed page to extend the file
        let page = Page::new(id);
        self.write_page_to_disk(&page)?;
        self.cache.insert(id.as_u64(), page);
        Ok(id)
    }

    /// Read a page (from cache or disk)
    pub fn read_page(&mut self, id: PageId) -> Result<&Page> {
        if id.as_u64() >= self.page_count {
            return Err(MiniDbError::Storage(format!(
                "page id {} out of range (have {})",
                id.as_u64(),
                self.page_count
            )));
        }

        if !self.cache.contains_key(&id.as_u64()) {
            let page = self.read_page_from_disk(id)?;
            self.cache.insert(id.as_u64(), page);
        }

        Ok(self.cache.get(&id.as_u64()).unwrap())
    }

    /// Read a page mutably (from cache or disk)
    pub fn read_page_mut(&mut self, id: PageId) -> Result<&mut Page> {
        if id.as_u64() >= self.page_count {
            return Err(MiniDbError::Storage(format!(
                "page id {} out of range",
                id.as_u64()
            )));
        }

        if !self.cache.contains_key(&id.as_u64()) {
            let page = self.read_page_from_disk(id)?;
            self.cache.insert(id.as_u64(), page);
        }

        Ok(self.cache.get_mut(&id.as_u64()).unwrap())
    }

    fn read_page_from_disk(&mut self, id: PageId) -> Result<Page> {
        let offset = id.as_u64() * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; PAGE_SIZE];
        self.file.read_exact(&mut buf)?;
        Ok(Page::from_bytes(id, &buf))
    }

    fn write_page_to_disk(&mut self, page: &Page) -> Result<()> {
        let offset = page.id.as_u64() * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(page.as_slice())?;
        Ok(())
    }

    /// Write a dirty page back to disk
    pub fn flush_page(&mut self, id: PageId) -> Result<()> {
        if let Some(page) = self.cache.get_mut(&id.as_u64()) {
            if page.dirty {
                let offset = id.as_u64() * PAGE_SIZE as u64;
                self.file.seek(SeekFrom::Start(offset))?;
                self.file.write_all(page.as_slice())?;
                page.dirty = false;
            }
        }
        Ok(())
    }

    /// Flush all dirty pages to disk
    pub fn flush_all(&mut self) -> Result<()> {
        let dirty_ids: Vec<u64> = self
            .cache
            .values()
            .filter(|p| p.dirty)
            .map(|p| p.id.as_u64())
            .collect();

        for id in dirty_ids {
            let page = self.cache.get_mut(&id).unwrap();
            let offset = id * PAGE_SIZE as u64;
            self.file.seek(SeekFrom::Start(offset))?;
            self.file.write_all(page.as_slice())?;
            page.dirty = false;
        }

        self.file.flush()?;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<()> {
        self.flush_all()?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn get_page_data(&self, id: PageId) -> Option<&[u8]> {
        self.cache.get(&id.as_u64()).map(|p| p.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_allocate_and_read() {
        let tmp = NamedTempFile::new().unwrap();
        let mut pager = Pager::open(tmp.path()).unwrap();

        let id = pager.allocate_page().unwrap();
        assert_eq!(id, PageId(0));

        let page = pager.read_page_mut(id).unwrap();
        page.data[0] = 42;
        page.mark_dirty();

        pager.flush_all().unwrap();

        let mut pager2 = Pager::open(tmp.path()).unwrap();
        let page2 = pager2.read_page(id).unwrap();
        assert_eq!(page2.data[0], 42);
    }

    #[test]
    fn test_multiple_pages() {
        let tmp = NamedTempFile::new().unwrap();
        let mut pager = Pager::open(tmp.path()).unwrap();

        for i in 0..5u8 {
            let id = pager.allocate_page().unwrap();
            let page = pager.read_page_mut(id).unwrap();
            page.data[0] = i;
            page.mark_dirty();
        }

        pager.flush_all().unwrap();
        assert_eq!(pager.page_count(), 5);
    }
}

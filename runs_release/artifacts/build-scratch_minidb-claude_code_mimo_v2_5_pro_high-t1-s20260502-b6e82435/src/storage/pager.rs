use crate::error::{MiniDbError, Result};
use crate::storage::page::{Page, PageId, PAGE_SIZE};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const MAX_CACHED_PAGES: usize = 256;

pub struct Pager {
    file: File,
    cache: HashMap<u64, Page>,
    page_count: u64,
    path: String,
}

impl Pager {
    pub fn open(path: &str) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let file_len = file.metadata()?.len();
        let page_count = if file_len == 0 {
            0
        } else {
            file_len / PAGE_SIZE as u64
        };

        Ok(Self {
            file,
            cache: HashMap::new(),
            page_count,
            path: path.to_string(),
        })
    }

    pub fn create(path: &str) -> Result<Self> {
        if Path::new(path).exists() {
            std::fs::remove_file(path)?;
        }
        Self::open(path)
    }

    pub fn allocate_page(&mut self) -> Result<PageId> {
        let id = PageId(self.page_count);
        self.page_count += 1;
        let page = Page::new(id);
        self.cache.insert(id.0, page);
        Ok(id)
    }

    pub fn read_page(&mut self, id: PageId) -> Result<&Page> {
        if !self.cache.contains_key(&id.0) {
            self.load_page_from_disk(id)?;
        }
        Ok(self.cache.get(&id.0).unwrap())
    }

    pub fn read_page_mut(&mut self, id: PageId) -> Result<&mut Page> {
        if !self.cache.contains_key(&id.0) {
            self.load_page_from_disk(id)?;
        }
        let page = self.cache.get_mut(&id.0).unwrap();
        page.mark_dirty();
        Ok(page)
    }

    pub fn get_page(&self, id: PageId) -> Option<&Page> {
        self.cache.get(&id.0)
    }

    pub fn get_page_mut(&mut self, id: PageId) -> Option<&mut Page> {
        self.cache.get_mut(&id.0)
    }

    fn load_page_from_disk(&mut self, id: PageId) -> Result<()> {
        if id.0 >= self.page_count {
            return Err(MiniDbError::Storage(format!(
                "page {} does not exist (page count: {})",
                id.0, self.page_count
            )));
        }

        let offset = id.0 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;

        let mut page = Page::new(id);
        self.file.read_exact(&mut page.data)?;

        // Evict if cache is full
        if self.cache.len() >= MAX_CACHED_PAGES {
            self.evict_one()?;
        }

        self.cache.insert(id.0, page);
        Ok(())
    }

    fn evict_one(&mut self) -> Result<()> {
        // Find a non-dirty page to evict (LRU-like: just pick the first non-dirty)
        let victim = self
            .cache
            .iter()
            .find(|(_, page)| !page.dirty)
            .map(|(id, _)| *id);

        if let Some(id) = victim {
            self.cache.remove(&id);
        } else {
            // All pages dirty - flush first one
            if let Some((&id, _)) = self.cache.iter().next() {
                let page = self.cache.remove(&id).unwrap();
                self.write_page_to_disk(&page)?;
            }
        }
        Ok(())
    }

    fn write_page_to_disk(&mut self, page: &Page) -> Result<()> {
        let offset = page.id.0 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(&page.data)?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        let ids: Vec<u64> = self
            .cache
            .iter()
            .filter(|(_, page)| page.dirty)
            .map(|(id, _)| *id)
            .collect();

        for id in ids {
            if let Some(mut page) = self.cache.remove(&id) {
                self.write_page_to_disk(&page)?;
                page.dirty = false;
                self.cache.insert(id, page);
            }
        }

        self.file.flush()?;
        Ok(())
    }

    pub fn page_count(&self) -> u64 {
        self.page_count
    }

    /// Clear the page cache, forcing pages to be re-read from disk on next access.
    /// Used for transaction rollback to restore committed state.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    pub fn sync(&mut self) -> Result<()> {
        self.flush()?;
        self.file.sync_data()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_pager_allocate_and_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_str().unwrap();

        let mut pager = Pager::create(path_str).unwrap();
        let id = pager.allocate_page().unwrap();

        {
            let page = pager.read_page_mut(id).unwrap();
            page.write_u64(0, 42);
        }

        let page = pager.read_page(id).unwrap();
        assert_eq!(page.read_u64(0), 42);
    }

    #[test]
    fn test_pager_flush_and_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_str().unwrap();

        {
            let mut pager = Pager::create(path_str).unwrap();
            let id = pager.allocate_page().unwrap();
            {
                let page = pager.read_page_mut(id).unwrap();
                page.write_u64(0, 12345);
            }
            pager.flush().unwrap();
        }

        // Reopen and verify
        let mut pager = Pager::open(path_str).unwrap();
        assert_eq!(pager.page_count(), 1);
        let page = pager.read_page(PageId(0)).unwrap();
        assert_eq!(page.read_u64(0), 12345);
    }

    #[test]
    fn test_pager_multiple_pages() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_str().unwrap();

        let mut pager = Pager::create(path_str).unwrap();
        for i in 0..10 {
            let id = pager.allocate_page().unwrap();
            let page = pager.read_page_mut(id).unwrap();
            page.write_u64(0, i);
        }

        pager.flush().unwrap();

        for i in 0..10 {
            let page = pager.read_page(PageId(i)).unwrap();
            assert_eq!(page.read_u64(0), i);
        }
    }
}

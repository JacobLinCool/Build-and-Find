use crate::error::{MiniDbError, Result};
use crate::storage::page::{Page, PageId, PAGE_SIZE};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Page cache with simple LRU-ish eviction
pub struct Pager {
    file: File,
    cache: HashMap<u64, Page>,
    page_count: u64,
}

impl Pager {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let file_len = file.metadata()?.len();
        let page_count = file_len / PAGE_SIZE as u64;
        Ok(Pager {
            file,
            cache: HashMap::new(),
            page_count,
        })
    }

    pub fn page_count(&self) -> u64 {
        self.page_count
    }

    pub fn alloc_page(&mut self) -> Result<PageId> {
        let id = PageId(self.page_count);
        self.page_count += 1;
        let page = Page::new(id);
        self.cache.insert(id.0, page);
        Ok(id)
    }

    pub fn get_page(&mut self, id: PageId) -> Result<&mut Page> {
        if !self.cache.contains_key(&id.0) {
            let page = self.read_page(id)?;
            self.cache.insert(id.0, page);
        }
        Ok(self.cache.get_mut(&id.0).unwrap())
    }

    pub fn get_page_ref(&mut self, id: PageId) -> Result<&Page> {
        if !self.cache.contains_key(&id.0) {
            let page = self.read_page(id)?;
            self.cache.insert(id.0, page);
        }
        Ok(self.cache.get(&id.0).unwrap())
    }

    fn read_page(&mut self, id: PageId) -> Result<Page> {
        if id.0 >= self.page_count {
            return Err(MiniDbError::Storage(format!(
                "page {} out of range (count={})",
                id.0, self.page_count
            )));
        }
        let offset = id.0 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        let mut page = Page::new(id);
        self.file.read_exact(&mut *page.data)?;
        Ok(page)
    }

    pub fn write_page(&mut self, id: PageId) -> Result<()> {
        let page = self
            .cache
            .get(&id.0)
            .ok_or_else(|| MiniDbError::Storage(format!("page {} not in cache", id.0)))?;
        let offset = id.0 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(&*page.data)?;
        if let Some(p) = self.cache.get_mut(&id.0) {
            p.dirty = false;
        }
        Ok(())
    }

    pub fn flush_all(&mut self) -> Result<()> {
        let dirty: Vec<u64> = self
            .cache
            .values()
            .filter(|p| p.dirty)
            .map(|p| p.id.0)
            .collect();
        for id in dirty {
            self.write_page(PageId(id))?;
        }
        self.file.flush()?;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<()> {
        self.flush_all()?;
        self.file.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_alloc_and_readwrite() {
        let tmp = NamedTempFile::new().unwrap();
        let mut pager = Pager::open(tmp.path()).unwrap();
        let id = pager.alloc_page().unwrap();
        {
            let page = pager.get_page(id).unwrap();
            page.data[0] = 42;
            page.mark_dirty();
        }
        pager.flush_all().unwrap();

        let mut pager2 = Pager::open(tmp.path()).unwrap();
        let page = pager2.get_page(id).unwrap();
        assert_eq!(page.data[0], 42);
    }
}

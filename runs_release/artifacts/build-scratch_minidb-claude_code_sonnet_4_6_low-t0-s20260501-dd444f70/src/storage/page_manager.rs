use crate::error::{MiniDbError, Result};
use crate::storage::page::{Page, PageId, PAGE_SIZE};
use log::trace;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub struct PageManager {
    file: File,
    cache: HashMap<u64, Page>,
    pub num_pages: u64,
}

impl PageManager {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let metadata = file.metadata()?;
        let num_pages = metadata.len() / PAGE_SIZE as u64;
        Ok(PageManager {
            file,
            cache: HashMap::new(),
            num_pages,
        })
    }

    pub fn read_page(&mut self, pid: PageId) -> Result<&mut Page> {
        if !self.cache.contains_key(&pid.0) {
            let mut page = Page::new(pid);
            if pid.0 < self.num_pages {
                let offset = pid.as_file_offset();
                self.file.seek(SeekFrom::Start(offset))?;
                self.file.read_exact(page.data.as_mut())?;
                trace!("read page {}", pid.0);
            }
            self.cache.insert(pid.0, page);
        }
        Ok(self.cache.get_mut(&pid.0).unwrap())
    }

    pub fn alloc_page(&mut self) -> Result<PageId> {
        let pid = PageId(self.num_pages);
        self.num_pages += 1;
        let page = Page::new(pid);
        self.cache.insert(pid.0, page);
        Ok(pid)
    }

    pub fn write_page(&mut self, pid: PageId, data: &[u8; PAGE_SIZE]) -> Result<()> {
        let offset = pid.as_file_offset();
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(data)?;
        if pid.0 >= self.num_pages {
            self.num_pages = pid.0 + 1;
        }
        trace!("wrote page {}", pid.0);
        Ok(())
    }

    pub fn flush_dirty_pages(&mut self) -> Result<()> {
        let dirty: Vec<u64> = self
            .cache
            .values()
            .filter(|p| p.dirty)
            .map(|p| p.id.0)
            .collect();
        for pid_num in dirty {
            let data = *self.cache[&pid_num].data.clone();
            let offset = pid_num * PAGE_SIZE as u64;
            self.file.seek(SeekFrom::Start(offset))?;
            self.file.write_all(&data)?;
            if let Some(page) = self.cache.get_mut(&pid_num) {
                page.dirty = false;
            }
        }
        self.file.flush()?;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<()> {
        self.flush_dirty_pages()?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn invalidate_cache(&mut self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_alloc_and_write_read() {
        let tmp = NamedTempFile::new().unwrap();
        let mut pm = PageManager::open(tmp.path()).unwrap();

        let pid = pm.alloc_page().unwrap();
        {
            let page = pm.read_page(pid).unwrap();
            page.data[0] = 42;
            page.dirty = true;
        }
        pm.flush_dirty_pages().unwrap();

        // Reopen
        let mut pm2 = PageManager::open(tmp.path()).unwrap();
        let page2 = pm2.read_page(pid).unwrap();
        assert_eq!(page2.data[0], 42);
    }
}

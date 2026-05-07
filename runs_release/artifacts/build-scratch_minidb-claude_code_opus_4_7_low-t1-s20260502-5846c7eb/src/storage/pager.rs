use crate::error::{MiniDbError, Result};
use crate::storage::page::{Page, PageId, PAGE_SIZE};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Magic at start of the meta page identifying a minidb file.
pub const MAGIC: &[u8; 8] = b"MiniDB01";

/// Pager: a thin abstraction over a file of fixed-size pages.
/// Pages are cached in memory; `flush` writes dirty pages and fsyncs.
pub struct Pager {
    file: File,
    path: PathBuf,
    cache: HashMap<PageId, Page>,
    dirty: HashMap<PageId, bool>,
    next_page: u64,
}

impl Pager {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;
        let len = file.metadata()?.len();
        let next_page = if len == 0 {
            1 // page 0 is meta
        } else {
            len / PAGE_SIZE as u64
        };
        if len == 0 {
            // Initialize meta page
            let mut meta = Page::empty(PageId::META);
            meta.data[..MAGIC.len()].copy_from_slice(MAGIC);
            meta.data[MAGIC.len()..MAGIC.len() + 8].copy_from_slice(&1u64.to_le_bytes());
            file.write_all(&meta.data)?;
            file.flush()?;
        } else {
            // Validate magic
            let mut buf = [0u8; 8];
            file.seek(SeekFrom::Start(0))?;
            file.read_exact(&mut buf)?;
            if &buf != MAGIC {
                return Err(MiniDbError::Storage(format!(
                    "not a minidb file: {}",
                    path.display()
                )));
            }
        }
        Ok(Pager {
            file,
            path,
            cache: HashMap::new(),
            dirty: HashMap::new(),
            next_page,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn allocate_page(&mut self) -> Result<PageId> {
        let id = PageId(self.next_page);
        self.next_page += 1;
        let page = Page::empty(id);
        self.cache.insert(id, page);
        self.dirty.insert(id, true);
        Ok(id)
    }

    pub fn read_page(&mut self, id: PageId) -> Result<&Page> {
        if !self.cache.contains_key(&id) {
            let mut buf = vec![0u8; PAGE_SIZE];
            let offset = id.raw() * PAGE_SIZE as u64;
            self.file.seek(SeekFrom::Start(offset))?;
            self.file.read_exact(&mut buf).map_err(|e| {
                MiniDbError::Storage(format!("failed to read page {}: {}", id.raw(), e))
            })?;
            self.cache.insert(id, Page { id, data: buf });
        }
        Ok(self.cache.get(&id).unwrap())
    }

    pub fn write_page(&mut self, page: Page) -> Result<()> {
        let id = page.id;
        self.cache.insert(id, page);
        self.dirty.insert(id, true);
        if id.raw() + 1 > self.next_page {
            self.next_page = id.raw() + 1;
        }
        Ok(())
    }

    pub fn page_mut(&mut self, id: PageId) -> Result<&mut Page> {
        // Ensure loaded
        let _ = self.read_page(id)?;
        self.dirty.insert(id, true);
        Ok(self.cache.get_mut(&id).unwrap())
    }

    pub fn flush(&mut self) -> Result<()> {
        // Pre-extend file if needed
        let needed_len = self.next_page * PAGE_SIZE as u64;
        let cur_len = self.file.metadata()?.len();
        if cur_len < needed_len {
            self.file.set_len(needed_len)?;
        }
        let dirty: Vec<PageId> = self
            .dirty
            .iter()
            .filter_map(|(k, v)| if *v { Some(*k) } else { None })
            .collect();
        for id in dirty {
            if let Some(page) = self.cache.get(&id) {
                let off = id.raw() * PAGE_SIZE as u64;
                self.file.seek(SeekFrom::Start(off))?;
                self.file.write_all(&page.data)?;
                self.dirty.insert(id, false);
            }
        }
        self.file.sync_all()?;
        Ok(())
    }

    pub fn next_page_id(&self) -> u64 {
        self.next_page
    }

    /// Reset the page allocator to a previous high-water mark. Intended for
    /// rollback of pages allocated within an aborted transaction.
    pub fn reset_next_page(&mut self, n: u64) {
        self.next_page = n;
    }

    /// Discard cached state (used to revert in-memory pages after a failed apply).
    pub fn discard_cache(&mut self) {
        self.cache.clear();
        self.dirty.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn alloc_and_read_back() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let mut p = Pager::open(&path).unwrap();
            let id = p.allocate_page().unwrap();
            let pg = p.page_mut(id).unwrap();
            pg.data[100] = 0xab;
            p.flush().unwrap();
        }
        let mut p = Pager::open(&path).unwrap();
        let pg = p.read_page(PageId(1)).unwrap();
        assert_eq!(pg.data[100], 0xab);
    }
}

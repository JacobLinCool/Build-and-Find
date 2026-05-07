use crate::error::{MiniDbError, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const PAGE_SIZE: usize = 4096;
pub const PAGE_HEADER_SIZE: usize = 20;
pub const SLOT_SIZE: usize = 4;

pub const PAGE_TYPE_FREE: u8 = 0;
pub const PAGE_TYPE_HEAP: u8 = 1;
pub const PAGE_TYPE_BTREE_INTERNAL: u8 = 2;
pub const PAGE_TYPE_BTREE_LEAF: u8 = 3;
pub const PAGE_TYPE_META: u8 = 4;

/// Identifies a page in the database file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageId(pub u64);

impl PageId {
    pub fn null() -> Self {
        PageId(u64::MAX)
    }
    pub fn is_null(self) -> bool {
        self.0 == u64::MAX
    }
}

/// A raw 4096-byte page.
pub struct Page {
    pub data: [u8; PAGE_SIZE],
}

impl Page {
    pub fn new() -> Self {
        Page {
            data: [0u8; PAGE_SIZE],
        }
    }

    pub fn page_type(&self) -> u8 {
        self.data[0]
    }

    pub fn set_page_type(&mut self, t: u8) {
        self.data[0] = t;
    }

    pub fn flags(&self) -> u8 {
        self.data[1]
    }

    pub fn num_slots(&self) -> u16 {
        u16::from_le_bytes([self.data[2], self.data[3]])
    }

    pub fn set_num_slots(&mut self, n: u16) {
        let b = n.to_le_bytes();
        self.data[2] = b[0];
        self.data[3] = b[1];
    }

    pub fn free_start(&self) -> u16 {
        u16::from_le_bytes([self.data[4], self.data[5]])
    }

    pub fn set_free_start(&mut self, v: u16) {
        let b = v.to_le_bytes();
        self.data[4] = b[0];
        self.data[5] = b[1];
    }

    pub fn free_end(&self) -> u16 {
        u16::from_le_bytes([self.data[6], self.data[7]])
    }

    pub fn set_free_end(&mut self, v: u16) {
        let b = v.to_le_bytes();
        self.data[6] = b[0];
        self.data[7] = b[1];
    }

    pub fn next_page(&self) -> PageId {
        let id = u64::from_le_bytes(self.data[8..16].try_into().unwrap());
        PageId(id)
    }

    pub fn set_next_page(&mut self, id: PageId) {
        let b = id.0.to_le_bytes();
        self.data[8..16].copy_from_slice(&b);
    }

    /// Initialize as a heap page.
    pub fn init_heap(&mut self) {
        self.data = [0u8; PAGE_SIZE];
        self.data[0] = PAGE_TYPE_HEAP;
        // free_start starts after header
        let start = PAGE_HEADER_SIZE as u16;
        self.set_free_start(start);
        self.set_free_end(PAGE_SIZE as u16);
        self.set_next_page(PageId::null());
    }

    /// Get slot entry at index i.
    pub fn slot(&self, i: usize) -> (u16, u16) {
        let offset = PAGE_HEADER_SIZE + i * SLOT_SIZE;
        let slot_off = u16::from_le_bytes([self.data[offset], self.data[offset + 1]]);
        let slot_len = u16::from_le_bytes([self.data[offset + 2], self.data[offset + 3]]);
        (slot_off, slot_len)
    }

    /// Set slot entry at index i.
    pub fn set_slot(&mut self, i: usize, offset: u16, length: u16) {
        let pos = PAGE_HEADER_SIZE + i * SLOT_SIZE;
        let ob = offset.to_le_bytes();
        let lb = length.to_le_bytes();
        self.data[pos] = ob[0];
        self.data[pos + 1] = ob[1];
        self.data[pos + 2] = lb[0];
        self.data[pos + 3] = lb[1];
    }

    /// Available free bytes for a new slot + data.
    pub fn available_space(&self) -> usize {
        let free_start = self.free_start() as usize;
        let free_end = self.free_end() as usize;
        if free_end > free_start + SLOT_SIZE {
            free_end - free_start - SLOT_SIZE
        } else {
            0
        }
    }

    /// Insert raw bytes into this page. Returns slot index, or Err if no space.
    pub fn insert_record(&mut self, data: &[u8]) -> Result<usize> {
        let data_len = data.len();
        if data_len + SLOT_SIZE > self.available_space() {
            return Err(MiniDbError::Internal("page full".into()));
        }
        let new_free_end = self.free_end() as usize - data_len;
        // Copy data to end of page
        self.data[new_free_end..new_free_end + data_len].copy_from_slice(data);

        // Add slot
        let slot_idx = self.num_slots() as usize;
        self.set_slot(slot_idx, new_free_end as u16, data_len as u16);
        self.set_num_slots(slot_idx as u16 + 1);
        self.set_free_start(self.free_start() + SLOT_SIZE as u16);
        self.set_free_end(new_free_end as u16);

        Ok(slot_idx)
    }

    /// Get record bytes for slot i. Returns None if slot is deleted.
    pub fn get_record(&self, slot: usize) -> Option<&[u8]> {
        if slot >= self.num_slots() as usize {
            return None;
        }
        let (off, len) = self.slot(slot);
        if len == 0 {
            return None; // deleted
        }
        Some(&self.data[off as usize..off as usize + len as usize])
    }

    /// Mark a slot as deleted.
    pub fn delete_record(&mut self, slot: usize) {
        if slot < self.num_slots() as usize {
            let (off, _) = self.slot(slot);
            self.set_slot(slot, off, 0);
        }
    }

    /// Replace record at slot with new data (in-place if same size, else error).
    pub fn update_record(&mut self, slot: usize, data: &[u8]) -> Result<()> {
        if slot >= self.num_slots() as usize {
            return Err(MiniDbError::Internal("slot out of range".into()));
        }
        let (off, len) = self.slot(slot);
        if len == 0 {
            return Err(MiniDbError::Internal("slot is deleted".into()));
        }
        if data.len() != len as usize {
            return Err(MiniDbError::Internal("update_record: size mismatch".into()));
        }
        self.data[off as usize..off as usize + len as usize].copy_from_slice(data);
        Ok(())
    }
}

/// File-backed page manager.
pub struct PageManager {
    file: File,
    pub num_pages: u64,
}

impl PageManager {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let file_len = file.metadata()?.len();
        let num_pages = file_len / PAGE_SIZE as u64;
        Ok(PageManager { file, num_pages })
    }

    /// Allocate a new page and return its ID.
    pub fn alloc_page(&mut self) -> Result<PageId> {
        let id = PageId(self.num_pages);
        self.num_pages += 1;
        // Write blank page
        let page = Page::new();
        self.write_page(id, &page)?;
        Ok(id)
    }

    pub fn read_page(&mut self, id: PageId) -> Result<Page> {
        if id.0 >= self.num_pages {
            return Err(MiniDbError::Internal(format!(
                "page {} out of range (have {})",
                id.0, self.num_pages
            )));
        }
        let offset = id.0 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        let mut page = Page::new();
        self.file.read_exact(&mut page.data)?;
        Ok(page)
    }

    pub fn write_page(&mut self, id: PageId, page: &Page) -> Result<()> {
        let offset = id.0 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(&page.data)?;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<()> {
        self.file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_page_insert_get() {
        let mut page = Page::new();
        page.init_heap();

        let data = b"hello world";
        let slot = page.insert_record(data).unwrap();
        assert_eq!(slot, 0);

        let got = page.get_record(0).unwrap();
        assert_eq!(got, data);
    }

    #[test]
    fn test_page_delete() {
        let mut page = Page::new();
        page.init_heap();

        page.insert_record(b"row1").unwrap();
        page.insert_record(b"row2").unwrap();
        page.delete_record(0);

        assert!(page.get_record(0).is_none());
        assert_eq!(page.get_record(1).unwrap(), b"row2");
    }

    #[test]
    fn test_page_manager() {
        let tmp = NamedTempFile::new().unwrap();
        let mut pm = PageManager::open(tmp.path()).unwrap();
        let id = pm.alloc_page().unwrap();
        assert_eq!(id.0, 0);

        let mut page = Page::new();
        page.init_heap();
        page.insert_record(b"test").unwrap();
        pm.write_page(id, &page).unwrap();

        let read_back = pm.read_page(id).unwrap();
        assert_eq!(read_back.get_record(0).unwrap(), b"test");
    }
}

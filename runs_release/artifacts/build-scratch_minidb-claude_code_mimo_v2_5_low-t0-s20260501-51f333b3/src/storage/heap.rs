use crate::error::{MiniDbError, Result};
use crate::storage::page::{Page, PageId, PAGE_SIZE};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// A heap page stores rows sequentially.
/// Page layout:
///   [0..2]       num_rows (u16)
///   [2..4]       data_end (u16) - offset where next row data would be written
///   [4..]        row entries: for each row, [2 bytes len][row data bytes]
///                To find row i, scan entries from the beginning.
pub struct HeapFile {
    file: File,
    pages: HashMap<u64, Page>,
}

/// Metadata for a row stored in the page
#[derive(Debug, Clone, Copy)]
struct RowEntry {
    offset: u16,
    len: u16,
}

impl HeapFile {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)?;

        let mut heap = HeapFile {
            file,
            pages: HashMap::new(),
        };

        // Load existing pages
        heap.file.seek(SeekFrom::Start(0))?;
        let mut buf = vec![0u8; PAGE_SIZE];
        let mut page_id = 0u64;
        loop {
            match heap.file.read_exact(&mut buf) {
                Ok(()) => {
                    let mut page = Page::new(PageId(page_id));
                    page.data.copy_from_slice(&buf);
                    heap.pages.insert(page_id, page);
                    page_id += 1;
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
        }

        Ok(heap)
    }

    pub fn load_page(&mut self, page_id: u64) -> Result<Page> {
        if let Some(page) = self.pages.get(&page_id) {
            return Ok(page.clone());
        }

        let offset = page_id * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;

        let mut page = Page::new(PageId(page_id));
        match self.file.read_exact(&mut page.data) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // Page doesn't exist yet, return empty page
            }
            Err(e) => return Err(e.into()),
        }

        self.pages.insert(page_id, page.clone());
        Ok(page)
    }

    pub fn write_page(&mut self, page: &Page) -> Result<()> {
        let offset = page.id.offset();
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(&page.data)?;
        self.file.sync_all()?;
        self.pages.insert(page.id.0, page.clone());
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        for page in self.pages.values() {
            if page.dirty {
                let offset = page.id.offset();
                self.file.seek(SeekFrom::Start(offset))?;
                self.file.write_all(&page.data)?;
            }
        }
        self.file.sync_all()?;
        for page in self.pages.values_mut() {
            page.dirty = false;
        }
        Ok(())
    }

    /// Read the list of row entries from a page header.
    fn read_entries(&self, page: &Page) -> Vec<RowEntry> {
        let num_rows = page.read_u16(0) as usize;
        let data_end = page.read_u16(2) as usize;

        let mut entries = Vec::new();
        let mut offset = 4usize;
        for _ in 0..num_rows {
            if offset + 4 > data_end {
                break;
            }
            let len = page.read_u16(offset) as usize;
            let row_offset = offset + 2;
            entries.push(RowEntry {
                offset: row_offset as u16,
                len: len as u16,
            });
            offset = row_offset + len;
        }
        entries
    }

    pub fn insert_row(&mut self, page_id: u64, row_data: &[u8]) -> Result<u16> {
        let mut page = self.load_page(page_id)?;

        let num_rows = page.read_u16(0) as usize;
        let mut data_end = page.read_u16(2) as usize;

        // Data starts after the 4-byte header
        if data_end == 0 {
            data_end = 4;
        }

        // New entry: 2 bytes for length + row data
        let entry_size = 2 + row_data.len();
        let new_data_end = data_end + entry_size;

        if new_data_end > PAGE_SIZE {
            return Err(MiniDbError::StorageError("page full".to_string()));
        }

        // Write the length prefix
        page.write_u16(data_end, row_data.len() as u16);
        // Write the row data
        page.write_bytes(data_end + 2, row_data);

        // Update header
        page.write_u16(0, (num_rows + 1) as u16);
        page.write_u16(2, new_data_end as u16);

        self.write_page(&page)?;
        Ok(num_rows as u16)
    }

    pub fn get_row(&mut self, page_id: u64, slot: u16) -> Result<Vec<u8>> {
        let page = self.load_page(page_id)?;
        let entries = self.read_entries(&page);

        let entry = entries
            .get(slot as usize)
            .ok_or_else(|| MiniDbError::StorageError("invalid slot".to_string()))?;

        Ok(page
            .read_bytes(entry.offset as usize, entry.len as usize)
            .to_vec())
    }

    pub fn delete_row(&mut self, page_id: u64, slot: u16) -> Result<()> {
        let mut page = self.load_page(page_id)?;
        let entries = self.read_entries(&page);

        let entry = entries
            .get(slot as usize)
            .ok_or_else(|| MiniDbError::StorageError("invalid slot".to_string()))?;

        // Mark as deleted by zeroing the length
        let len_offset = entry.offset as usize - 2;
        page.write_u16(len_offset, 0);
        self.write_page(&page)?;
        Ok(())
    }

    pub fn num_rows(&mut self, page_id: u64) -> Result<usize> {
        let page = self.load_page(page_id)?;
        let entries = self.read_entries(&page);
        Ok(entries.len())
    }

    pub fn scan_page(&mut self, page_id: u64) -> Result<Vec<(u16, Vec<u8>)>> {
        let page = self.load_page(page_id)?;
        let entries = self.read_entries(&page);
        let mut rows = Vec::new();

        for (i, entry) in entries.iter().enumerate() {
            if entry.len > 0 {
                let data = page
                    .read_bytes(entry.offset as usize, entry.len as usize)
                    .to_vec();
                rows.push((i as u16, data));
            }
        }

        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_heap_insert_and_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.heap");

        let mut heap = HeapFile::open(&path).unwrap();
        let page_id = 0;

        // Create the page
        let page = Page::new(PageId(page_id));
        heap.write_page(&page).unwrap();

        // Insert a row
        let row_data = b"hello world";
        let slot = heap.insert_row(page_id, row_data).unwrap();
        assert_eq!(slot, 0);

        // Read it back
        let data = heap.get_row(page_id, 0).unwrap();
        assert_eq!(data, row_data);
    }

    #[test]
    fn test_heap_scan() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.heap");

        let mut heap = HeapFile::open(&path).unwrap();
        let page_id = 0;

        let page = Page::new(PageId(page_id));
        heap.write_page(&page).unwrap();

        heap.insert_row(page_id, b"row1").unwrap();
        heap.insert_row(page_id, b"row2").unwrap();
        heap.insert_row(page_id, b"row3").unwrap();

        let rows = heap.scan_page(page_id).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].1, b"row1");
        assert_eq!(rows[1].1, b"row2");
        assert_eq!(rows[2].1, b"row3");
    }
}

//! Page manager: turns a file into an array of fixed-size pages.
//!
//! The pager owns the data file. It can read, write, and allocate pages.
//! A simple free list (kept in the file header) recycles deleted pages.

use crate::error::{MiniDbError, Result};
use crate::storage::page::{PageId, PAGE_SIZE};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const FILE_MAGIC: &[u8; 8] = b"MINIDB01";

#[derive(Debug, Clone)]
pub struct Header {
    pub num_pages: u64,
    pub free_head: u64,
    pub catalog_root: u64,
    pub format_version: u32,
}

impl Default for Header {
    fn default() -> Self {
        Header {
            num_pages: 1,
            free_head: 0,
            catalog_root: 0,
            format_version: 1,
        }
    }
}

pub struct Pager {
    pub path: PathBuf,
    file: File,
    header: Header,
}

impl Pager {
    /// Open or create a paged file at `path`.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path: PathBuf = path.as_ref().into();
        let exists = path.exists();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        let mut p = Pager {
            path,
            file,
            header: Header::default(),
        };
        if exists && p.file_len()? >= PAGE_SIZE as u64 {
            p.read_header()?;
        } else {
            p.write_header()?;
            p.flush()?;
        }
        Ok(p)
    }

    fn file_len(&self) -> Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn header_mut(&mut self) -> &mut Header {
        &mut self.header
    }

    pub fn write_header(&mut self) -> Result<()> {
        let mut buf = vec![0u8; PAGE_SIZE];
        buf[0..8].copy_from_slice(FILE_MAGIC);
        buf[8..12].copy_from_slice(&self.header.format_version.to_le_bytes());
        buf[12..20].copy_from_slice(&self.header.num_pages.to_le_bytes());
        buf[20..28].copy_from_slice(&self.header.free_head.to_le_bytes());
        buf[28..36].copy_from_slice(&self.header.catalog_root.to_le_bytes());
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&buf)?;
        Ok(())
    }

    fn read_header(&mut self) -> Result<()> {
        let mut buf = vec![0u8; PAGE_SIZE];
        self.file.seek(SeekFrom::Start(0))?;
        self.file.read_exact(&mut buf)?;
        if &buf[0..8] != FILE_MAGIC {
            return Err(MiniDbError::storage("not a minidb file"));
        }
        let mut a = [0u8; 4];
        a.copy_from_slice(&buf[8..12]);
        self.header.format_version = u32::from_le_bytes(a);
        let mut b = [0u8; 8];
        b.copy_from_slice(&buf[12..20]);
        self.header.num_pages = u64::from_le_bytes(b);
        b.copy_from_slice(&buf[20..28]);
        self.header.free_head = u64::from_le_bytes(b);
        b.copy_from_slice(&buf[28..36]);
        self.header.catalog_root = u64::from_le_bytes(b);
        Ok(())
    }

    /// Read a page into a fresh 4096-byte buffer.
    pub fn read_page(&mut self, id: PageId) -> Result<Vec<u8>> {
        if id.0 == 0 || id.0 >= self.header.num_pages {
            return Err(MiniDbError::storage(format!("invalid page id: {}", id.0)));
        }
        let mut buf = vec![0u8; PAGE_SIZE];
        self.file.seek(SeekFrom::Start(id.0 * PAGE_SIZE as u64))?;
        self.file.read_exact(&mut buf)?;
        Ok(buf)
    }

    pub fn write_page(&mut self, id: PageId, data: &[u8]) -> Result<()> {
        if data.len() != PAGE_SIZE {
            return Err(MiniDbError::storage(format!(
                "page data must be {} bytes, got {}",
                PAGE_SIZE,
                data.len()
            )));
        }
        if id.0 == 0 || id.0 >= self.header.num_pages {
            return Err(MiniDbError::storage(format!(
                "invalid page id for write: {}",
                id.0
            )));
        }
        self.file.seek(SeekFrom::Start(id.0 * PAGE_SIZE as u64))?;
        self.file.write_all(data)?;
        Ok(())
    }

    /// Allocate a fresh page, either by popping the free list or by extending the file.
    pub fn allocate_page(&mut self) -> Result<PageId> {
        if self.header.free_head != 0 {
            let head = self.header.free_head;
            // The first 8 bytes of a free page hold the next free pointer.
            let mut buf = vec![0u8; PAGE_SIZE];
            self.file.seek(SeekFrom::Start(head * PAGE_SIZE as u64))?;
            self.file.read_exact(&mut buf)?;
            let mut nb = [0u8; 8];
            nb.copy_from_slice(&buf[0..8]);
            let next = u64::from_le_bytes(nb);
            self.header.free_head = next;
            // Zero out the page on reuse so callers don't see stale data.
            let zeros = vec![0u8; PAGE_SIZE];
            self.file.seek(SeekFrom::Start(head * PAGE_SIZE as u64))?;
            self.file.write_all(&zeros)?;
            self.write_header()?;
            return Ok(PageId(head));
        }
        let id = PageId(self.header.num_pages);
        self.header.num_pages += 1;
        // Extend file by one zeroed page.
        let zeros = vec![0u8; PAGE_SIZE];
        self.file.seek(SeekFrom::Start(id.0 * PAGE_SIZE as u64))?;
        self.file.write_all(&zeros)?;
        self.write_header()?;
        Ok(id)
    }

    /// Return a page to the free list.
    pub fn free_page(&mut self, id: PageId) -> Result<()> {
        if id.0 == 0 || id.0 >= self.header.num_pages {
            return Err(MiniDbError::storage(format!(
                "cannot free invalid page: {}",
                id.0
            )));
        }
        let mut buf = vec![0u8; PAGE_SIZE];
        buf[0..8].copy_from_slice(&self.header.free_head.to_le_bytes());
        self.write_page(id, &buf)?;
        self.header.free_head = id.0;
        self.write_header()?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.file.sync_data()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let mut p = temp_dir();
        p.push(format!("minidb_test_{}_{}.db", pid, n));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn allocate_and_read() {
        let path = tmp();
        let mut pager = Pager::open(&path).unwrap();
        let p1 = pager.allocate_page().unwrap();
        let p2 = pager.allocate_page().unwrap();
        assert_ne!(p1.0, p2.0);
        let mut data = vec![0u8; PAGE_SIZE];
        data[0..3].copy_from_slice(b"hi!");
        pager.write_page(p1, &data).unwrap();
        let got = pager.read_page(p1).unwrap();
        assert_eq!(&got[0..3], b"hi!");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn free_list_reuses() {
        let path = tmp();
        let mut pager = Pager::open(&path).unwrap();
        let p1 = pager.allocate_page().unwrap();
        let _p2 = pager.allocate_page().unwrap();
        let pages_before = pager.header().num_pages;
        pager.free_page(p1).unwrap();
        let p3 = pager.allocate_page().unwrap();
        assert_eq!(p3.0, p1.0);
        assert_eq!(pager.header().num_pages, pages_before);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn header_persists() {
        let path = tmp();
        {
            let mut pager = Pager::open(&path).unwrap();
            pager.header_mut().catalog_root = 42;
            pager.write_header().unwrap();
            pager.flush().unwrap();
        }
        let pager = Pager::open(&path).unwrap();
        assert_eq!(pager.header().catalog_root, 42);
        std::fs::remove_file(&path).ok();
    }
}

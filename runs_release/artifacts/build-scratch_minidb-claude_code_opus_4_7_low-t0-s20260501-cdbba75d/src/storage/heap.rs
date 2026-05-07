//! Page-based file manager for the database snapshot.
//!
//! Layout:
//! - Page 0: header — magic (8) | version (4) | num_pages (8) | snapshot_len (8) | reserved
//! - Pages 1..K: serialized snapshot bytes split into PAGE_SIZE chunks
//!
//! On commit, the in-memory database state is serialized (bincode) and written
//! page-aligned. On open, the file is read back into a single byte buffer.

use crate::error::{MiniDbError, Result};
use crate::storage::page::{Page, PageId, PAGE_SIZE};
use log::debug;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 8] = b"MINIDBv1";
pub const FORMAT_VERSION: u32 = 1;

pub struct PageManager {
    path: PathBuf,
    file: File,
    pub num_pages: u64,
    pub snapshot_len: u64,
}

impl PageManager {
    pub fn open(path: &Path) -> Result<Self> {
        let exists = path.exists();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let mut pm = PageManager {
            path: path.to_path_buf(),
            file,
            num_pages: 0,
            snapshot_len: 0,
        };
        if exists {
            pm.read_header()?;
        } else {
            pm.write_empty_header()?;
        }
        Ok(pm)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read_header(&mut self) -> Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut buf = [0u8; PAGE_SIZE];
        let n = read_full(&mut self.file, &mut buf)?;
        if n < 28 {
            return Err(MiniDbError::Storage("file too small for header".into()));
        }
        if &buf[0..8] != MAGIC {
            return Err(MiniDbError::Storage("bad magic".into()));
        }
        let ver = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        if ver != FORMAT_VERSION {
            return Err(MiniDbError::Storage(format!(
                "unsupported format version {}",
                ver
            )));
        }
        self.num_pages = u64::from_le_bytes(buf[12..20].try_into().unwrap());
        self.snapshot_len = u64::from_le_bytes(buf[20..28].try_into().unwrap());
        Ok(())
    }

    fn write_empty_header(&mut self) -> Result<()> {
        self.num_pages = 1;
        self.snapshot_len = 0;
        self.write_header()
    }

    fn write_header(&mut self) -> Result<()> {
        let mut buf = [0u8; PAGE_SIZE];
        buf[0..8].copy_from_slice(MAGIC);
        buf[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        buf[12..20].copy_from_slice(&self.num_pages.to_le_bytes());
        buf[20..28].copy_from_slice(&self.snapshot_len.to_le_bytes());
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&buf)?;
        self.file.sync_data()?;
        Ok(())
    }

    pub fn read_page(&mut self, id: PageId) -> Result<Page> {
        let offset = id.0 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        let mut data = vec![0u8; PAGE_SIZE];
        read_full(&mut self.file, &mut data)?;
        Ok(Page { id, data })
    }

    pub fn write_page(&mut self, page: &Page) -> Result<()> {
        if page.data.len() != PAGE_SIZE {
            return Err(MiniDbError::Storage("invalid page size".into()));
        }
        let offset = page.id.0 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(&page.data)?;
        Ok(())
    }

    /// Persist the in-memory snapshot to pages 1..K and update header.
    pub fn write_snapshot(&mut self, bytes: &[u8]) -> Result<()> {
        let n_pages = ((bytes.len() + PAGE_SIZE - 1) / PAGE_SIZE).max(0);
        debug!(
            "writing snapshot: {} bytes across {} pages",
            bytes.len(),
            n_pages
        );
        for i in 0..n_pages {
            let start = i * PAGE_SIZE;
            let end = ((i + 1) * PAGE_SIZE).min(bytes.len());
            let mut page = Page::empty(PageId(1 + i as u64));
            page.data[..end - start].copy_from_slice(&bytes[start..end]);
            self.write_page(&page)?;
        }
        self.snapshot_len = bytes.len() as u64;
        self.num_pages = 1 + n_pages as u64;
        // truncate file to current pages
        self.file.set_len(self.num_pages * PAGE_SIZE as u64)?;
        self.write_header()?;
        self.file.sync_data()?;
        Ok(())
    }

    pub fn read_snapshot(&mut self) -> Result<Vec<u8>> {
        if self.snapshot_len == 0 {
            return Ok(Vec::new());
        }
        let mut out = vec![0u8; self.snapshot_len as usize];
        let n_pages = ((self.snapshot_len as usize + PAGE_SIZE - 1) / PAGE_SIZE) as u64;
        for i in 0..n_pages {
            let page = self.read_page(PageId(1 + i))?;
            let start = (i as usize) * PAGE_SIZE;
            let end = ((i as usize + 1) * PAGE_SIZE).min(out.len());
            out[start..end].copy_from_slice(&page.data[..end - start]);
        }
        Ok(out)
    }
}

fn read_full(file: &mut File, buf: &mut [u8]) -> Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match file.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn _td() -> tempfile::TempDir {
        tempdir().expect("tempdir")
    }

    #[test]
    fn open_and_persist_snapshot() {
        let d = _td();
        let p = d.path().join("a.db");
        {
            let mut pm = PageManager::open(&p).unwrap();
            assert_eq!(pm.num_pages, 1);
            pm.write_snapshot(b"hello world").unwrap();
        }
        let mut pm = PageManager::open(&p).unwrap();
        assert_eq!(pm.read_snapshot().unwrap(), b"hello world");
    }

    #[test]
    fn write_then_read_multipage_snapshot() {
        let d = _td();
        let p = d.path().join("b.db");
        let big: Vec<u8> = (0..10_000).map(|i| (i % 251) as u8).collect();
        {
            let mut pm = PageManager::open(&p).unwrap();
            pm.write_snapshot(&big).unwrap();
        }
        let mut pm = PageManager::open(&p).unwrap();
        assert_eq!(pm.read_snapshot().unwrap(), big);
    }
}

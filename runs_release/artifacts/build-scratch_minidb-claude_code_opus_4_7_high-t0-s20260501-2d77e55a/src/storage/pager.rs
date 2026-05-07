//! Page manager: reads, writes, and allocates 4 KiB pages backed by a single file.
//!
//! The pager maintains an in-memory buffer of dirty pages so that uncommitted
//! changes can be discarded on rollback or persisted on commit.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use log::debug;

use crate::error::{MiniDbError, Result};

use super::page::{PageId, PAGE_SIZE};

/// A 4 KiB byte buffer used as a unit of disk I/O.
pub type PageBuf = [u8; PAGE_SIZE];

pub struct Pager {
    path: PathBuf,
    file: File,
    /// Number of pages currently in the file (page ids 1..=num_pages are valid).
    num_pages: u64,
    /// Clean, recently read pages.
    cache: HashMap<PageId, Box<PageBuf>>,
    /// Dirty pages produced by the current transaction; flushed on commit.
    dirty: HashMap<PageId, Box<PageBuf>>,
}

impl Pager {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;
        let len = file.metadata()?.len();
        if len % PAGE_SIZE as u64 != 0 {
            return Err(MiniDbError::storage(format!(
                "data file {:?} length {} is not a multiple of {} bytes",
                path, len, PAGE_SIZE
            )));
        }
        Ok(Self {
            path,
            file,
            num_pages: len / PAGE_SIZE as u64,
            cache: HashMap::new(),
            dirty: HashMap::new(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn num_pages(&self) -> u64 {
        self.num_pages
    }

    /// Allocate a new page, returning its id and a zeroed buffer marked dirty.
    pub fn allocate(&mut self) -> PageId {
        self.num_pages += 1;
        let id = PageId(self.num_pages);
        let buf = Box::new([0u8; PAGE_SIZE]);
        self.dirty.insert(id, buf);
        debug!("allocated page {}", id);
        id
    }

    /// Read a page into a freshly-allocated buffer.
    pub fn read(&mut self, id: PageId) -> Result<Box<PageBuf>> {
        if id.is_null() || id.0 > self.num_pages {
            return Err(MiniDbError::storage(format!("page id {} out of range", id)));
        }
        if let Some(b) = self.dirty.get(&id) {
            return Ok(b.clone());
        }
        if let Some(b) = self.cache.get(&id) {
            return Ok(b.clone());
        }
        let mut buf = Box::new([0u8; PAGE_SIZE]);
        self.file
            .seek(SeekFrom::Start((id.0 - 1) * PAGE_SIZE as u64))?;
        self.file.read_exact(buf.as_mut_slice())?;
        self.cache.insert(id, buf.clone());
        Ok(buf)
    }

    /// Write a page (will be persisted on the next `flush_dirty`).
    pub fn write(&mut self, id: PageId, buf: Box<PageBuf>) -> Result<()> {
        if id.is_null() || id.0 > self.num_pages {
            return Err(MiniDbError::storage(format!(
                "page id {} not allocated",
                id
            )));
        }
        self.dirty.insert(id, buf);
        Ok(())
    }

    /// Flush dirty pages to disk and fsync.
    pub fn flush(&mut self) -> Result<()> {
        // Sort to make writes deterministic in tests.
        let mut ids: Vec<PageId> = self.dirty.keys().copied().collect();
        ids.sort();
        for id in ids {
            let buf = self.dirty.remove(&id).expect("present");
            self.file
                .seek(SeekFrom::Start((id.0 - 1) * PAGE_SIZE as u64))?;
            self.file.write_all(buf.as_slice())?;
            self.cache.insert(id, buf);
        }
        self.file.sync_all()?;
        Ok(())
    }

    /// Discard buffered modifications (rollback).
    pub fn discard_dirty(&mut self) {
        if !self.dirty.is_empty() {
            debug!("discarding {} dirty pages", self.dirty.len());
        }
        self.dirty.clear();
        // Truncate file back to the persisted size.
        if let Ok(meta) = self.file.metadata() {
            let on_disk_pages = meta.len() / PAGE_SIZE as u64;
            self.num_pages = on_disk_pages;
        }
    }

    pub fn has_pending_writes(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// Return the dirty pages snapshot (used by WAL writer at commit time).
    pub fn dirty_snapshot(&self) -> Vec<(PageId, Box<PageBuf>)> {
        let mut v: Vec<(PageId, Box<PageBuf>)> =
            self.dirty.iter().map(|(k, v)| (*k, v.clone())).collect();
        v.sort_by_key(|(k, _)| *k);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn allocate_write_read_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data");
        let mut p = Pager::open(&path).unwrap();
        let id = p.allocate();
        let mut buf = Box::new([0u8; PAGE_SIZE]);
        buf[0] = 42;
        p.write(id, buf).unwrap();
        p.flush().unwrap();
        // Reopen
        drop(p);
        let mut p = Pager::open(&path).unwrap();
        let got = p.read(id).unwrap();
        assert_eq!(got[0], 42);
    }

    #[test]
    fn discard_drops_dirty_pages() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data");
        let mut p = Pager::open(&path).unwrap();
        let _id = p.allocate();
        assert!(p.has_pending_writes());
        p.discard_dirty();
        assert!(!p.has_pending_writes());
    }
}

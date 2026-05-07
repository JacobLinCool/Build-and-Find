use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{MiniDbError, Result};

pub mod btree;
pub mod heap;
pub mod wal;

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageId(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotHeader {
    magic: [u8; 8],
    version: u32,
    snapshot_lsn: u64,
    payload_len: u64,
}

#[derive(Debug, Clone)]
pub struct PageManager {
    path: PathBuf,
}

impl PageManager {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn open_rw(&self) -> Result<File> {
        Ok(OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&self.path)?)
    }

    pub fn load_snapshot(&self) -> Result<Option<(u64, Vec<u8>)>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let mut file = self.open_rw()?;
        let mut header_bytes = vec![0u8; PAGE_SIZE];
        if file.read_exact(&mut header_bytes).is_err() {
            return Ok(None);
        }
        let header_len =
            u32::from_le_bytes(header_bytes[0..4].try_into().unwrap_or([0; 4])) as usize;
        if header_len == 0 || header_len > PAGE_SIZE - 4 {
            return Ok(None);
        }
        let header: SnapshotHeader = match bincode::deserialize(&header_bytes[4..4 + header_len]) {
            Ok(header) => header,
            Err(_) => return Ok(None),
        };
        if &header.magic != b"MINIDB\0\0" {
            return Ok(None);
        }
        let payload_len = header.payload_len as usize;
        let mut payload = vec![0u8; payload_len];
        let mut read = 0usize;
        while read < payload_len {
            let page_index = 1 + read / PAGE_SIZE;
            let mut page = vec![0u8; PAGE_SIZE];
            file.seek(SeekFrom::Start((page_index * PAGE_SIZE) as u64))?;
            let n = file.read(&mut page)?;
            if n == 0 {
                break;
            }
            let to_copy = (payload_len - read).min(PAGE_SIZE).min(n);
            payload[read..read + to_copy].copy_from_slice(&page[..to_copy]);
            read += to_copy;
        }
        payload.truncate(payload_len);
        Ok(Some((header.snapshot_lsn, payload)))
    }

    pub fn store_snapshot(&self, snapshot_lsn: u64, payload: &[u8]) -> Result<()> {
        let header = SnapshotHeader {
            magic: *b"MINIDB\0\0",
            version: 1,
            snapshot_lsn,
            payload_len: payload.len() as u64,
        };
        let serialized_header =
            bincode::serialize(&header).map_err(|err| MiniDbError::Storage(err.to_string()))?;
        let mut header_bytes = vec![0u8; PAGE_SIZE];
        let len = serialized_header.len().min(PAGE_SIZE - 4);
        header_bytes[0..4].copy_from_slice(&(len as u32).to_le_bytes());
        header_bytes[4..4 + len].copy_from_slice(&serialized_header[..len]);
        let mut file = self.open_rw()?;
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header_bytes)?;
        let mut offset = 0usize;
        while offset < payload.len() {
            let mut page = vec![0u8; PAGE_SIZE];
            let chunk = (payload.len() - offset).min(PAGE_SIZE);
            page[..chunk].copy_from_slice(&payload[offset..offset + chunk]);
            file.write_all(&page)?;
            offset += chunk;
        }
        file.flush()?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn page_manager_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("db");
        let pm = PageManager::new(&path);
        pm.store_snapshot(7, b"hello world").unwrap();
        let loaded = pm.load_snapshot().unwrap().unwrap();
        assert_eq!(loaded.0, 7);
        assert_eq!(loaded.1, b"hello world");
    }
}

//! Append-only, record-oriented write-ahead log with CRC32 per record.
//!
//! Each record on disk has the layout:
//!
//! ```text
//! | u32 LE length | u32 LE crc32 | payload bytes... |
//! ```
//!
//! The CRC covers only the payload. Corrupt tails are detected by either
//! a short read of the header or a CRC mismatch and stop recovery
//! gracefully (the remaining bytes are ignored).

use crate::error::{MiniDbError, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Identifies the semantic kind of a WAL record. Encoded as the first byte
/// of the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WalRecordKind {
    Begin = 1,
    Commit = 2,
    Rollback = 3,
    Insert = 10,
    Update = 11,
    Delete = 12,
    Checkpoint = 20,
}

impl WalRecordKind {
    pub fn from_byte(b: u8) -> Result<Self> {
        match b {
            1 => Ok(WalRecordKind::Begin),
            2 => Ok(WalRecordKind::Commit),
            3 => Ok(WalRecordKind::Rollback),
            10 => Ok(WalRecordKind::Insert),
            11 => Ok(WalRecordKind::Update),
            12 => Ok(WalRecordKind::Delete),
            20 => Ok(WalRecordKind::Checkpoint),
            other => Err(MiniDbError::Storage(format!(
                "unknown wal record kind {other}"
            ))),
        }
    }
}

/// Decoded record returned by [`Wal::replay`].
#[derive(Debug, Clone)]
pub struct WalRecord {
    pub kind: WalRecordKind,
    pub txn_id: u64,
    pub body: Vec<u8>,
}

/// Append-only write-ahead log. Thread-safe via an internal `Mutex` so the
/// transaction manager can use it from a shared reference.
pub struct Wal {
    path: PathBuf,
    inner: Mutex<File>,
}

impl Wal {
    /// Open (or create) the log file at `path` in append mode.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            path,
            inner: Mutex::new(file),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append a single record. Does not fsync — call [`Wal::fsync`] at
    /// commit time.
    pub fn append(&self, kind: WalRecordKind, txn_id: u64, body: &[u8]) -> Result<()> {
        let mut payload = Vec::with_capacity(1 + 8 + body.len());
        payload.push(kind as u8);
        payload.extend_from_slice(&txn_id.to_le_bytes());
        payload.extend_from_slice(body);

        let len = payload.len() as u32;
        let crc = crc32fast::hash(&payload);

        let mut guard = self
            .inner
            .lock()
            .map_err(|_| MiniDbError::Storage("wal mutex poisoned".into()))?;
        guard.write_all(&len.to_le_bytes())?;
        guard.write_all(&crc.to_le_bytes())?;
        guard.write_all(&payload)?;
        Ok(())
    }

    /// Durability barrier. Flushes userspace buffers and calls `fsync`.
    pub fn fsync(&self) -> Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| MiniDbError::Storage("wal mutex poisoned".into()))?;
        guard.flush()?;
        guard.sync_all()?;
        Ok(())
    }

    /// Read all valid records from the front of the log. Stops at the
    /// first truncated or CRC-invalid record.
    pub fn replay(&self) -> Result<Vec<WalRecord>> {
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(0))?;

        let mut out = Vec::new();
        loop {
            let mut hdr = [0u8; 8];
            match file.read_exact(&mut hdr) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
            let len = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
            let expected = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
            let mut payload = vec![0u8; len];
            if file.read_exact(&mut payload).is_err() {
                break;
            }
            let actual = crc32fast::hash(&payload);
            if actual != expected {
                break;
            }
            if payload.len() < 1 + 8 {
                break;
            }
            let kind = WalRecordKind::from_byte(payload[0])?;
            let mut tid = [0u8; 8];
            tid.copy_from_slice(&payload[1..9]);
            let txn_id = u64::from_le_bytes(tid);
            let body = payload[9..].to_vec();
            out.push(WalRecord {
                kind,
                txn_id,
                body,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_replay_roundtrip() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("minidb_wal_{}.log", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let wal = Wal::open(&p).unwrap();
        wal.append(WalRecordKind::Begin, 1, b"").unwrap();
        wal.append(WalRecordKind::Insert, 1, b"row-1").unwrap();
        wal.append(WalRecordKind::Commit, 1, b"").unwrap();
        wal.fsync().unwrap();
        let recs = wal.replay().unwrap();
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[1].body, b"row-1");
        std::fs::remove_file(&p).ok();
    }
}

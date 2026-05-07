//! Append-only write-ahead log.
//!
//! Frame layout (little-endian):
//!   [4 bytes payload-length][4 bytes CRC32 of payload][payload bytes]
//!
//! Records are bincode-serialized `WalRecord`s. CRC mismatch or short read
//! truncates replay at that point — any uncommitted suffix is discarded.

use crate::error::Result;
use crate::types::{Row, Value};
use crc32fast::Hasher;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalRecord {
    Begin {
        txn_id: u64,
    },
    Commit {
        txn_id: u64,
    },
    Rollback {
        txn_id: u64,
    },
    InsertRow {
        txn_id: u64,
        table: String,
        row_id: u64,
        row: Row,
    },
    DeleteRow {
        txn_id: u64,
        table: String,
        row_id: u64,
    },
    UpdateRow {
        txn_id: u64,
        table: String,
        row_id: u64,
        new_row: Row,
    },
    CreateTable {
        txn_id: u64,
        name: String,
        schema_bytes: Vec<u8>,
    },
    DropTable {
        txn_id: u64,
        name: String,
    },
    CreateIndex {
        txn_id: u64,
        name: String,
        table: String,
        column: String,
    },
    DropIndex {
        txn_id: u64,
        name: String,
    },
    /// Marker that the snapshot has been flushed and any preceding records can
    /// be discarded; recovery skips past these.
    SnapshotMarker {
        up_to_txn: u64,
    },
}

impl WalRecord {
    pub fn txn_id(&self) -> Option<u64> {
        match self {
            WalRecord::Begin { txn_id }
            | WalRecord::Commit { txn_id }
            | WalRecord::Rollback { txn_id }
            | WalRecord::InsertRow { txn_id, .. }
            | WalRecord::DeleteRow { txn_id, .. }
            | WalRecord::UpdateRow { txn_id, .. }
            | WalRecord::CreateTable { txn_id, .. }
            | WalRecord::DropTable { txn_id, .. }
            | WalRecord::CreateIndex { txn_id, .. }
            | WalRecord::DropIndex { txn_id, .. } => Some(*txn_id),
            WalRecord::SnapshotMarker { .. } => None,
        }
    }
}

pub struct Wal {
    path: PathBuf,
    file: File,
}

impl Wal {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .append(false)
            .open(path)?;
        Ok(Wal {
            path: path.to_path_buf(),
            file,
        })
    }

    pub fn append(&mut self, rec: &WalRecord) -> Result<()> {
        let payload = bincode::serialize(rec)?;
        let mut hasher = Hasher::new();
        hasher.update(&payload);
        let crc = hasher.finalize();
        let len = payload.len() as u32;
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&len.to_le_bytes())?;
        self.file.write_all(&crc.to_le_bytes())?;
        self.file.write_all(&payload)?;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<()> {
        self.file.sync_data()?;
        Ok(())
    }

    pub fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.sync_data()?;
        Ok(())
    }

    pub fn replay(&mut self) -> Result<Vec<WalRecord>> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut buf = Vec::new();
        self.file.read_to_end(&mut buf)?;
        let mut out = Vec::new();
        let mut i = 0usize;
        while i + 8 <= buf.len() {
            let len = u32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as usize;
            let crc = u32::from_le_bytes(buf[i + 4..i + 8].try_into().unwrap());
            if i + 8 + len > buf.len() {
                break; // torn write; truncate
            }
            let payload = &buf[i + 8..i + 8 + len];
            let mut h = Hasher::new();
            h.update(payload);
            if h.finalize() != crc {
                break;
            }
            let rec: WalRecord = match bincode::deserialize(payload) {
                Ok(r) => r,
                Err(_) => break,
            };
            out.push(rec);
            i += 8 + len;
        }
        Ok(out)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Convenience: locate the wal file alongside a database file.
pub fn wal_path_for(db_path: &Path) -> PathBuf {
    let mut s = db_path.as_os_str().to_owned();
    s.push(".wal");
    PathBuf::from(s)
}

#[allow(dead_code)]
fn _ensure_value_serializable(_v: &Value) {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn append_and_replay_roundtrip() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.wal");
        {
            let mut w = Wal::open(&p).unwrap();
            w.append(&WalRecord::Begin { txn_id: 1 }).unwrap();
            w.append(&WalRecord::InsertRow {
                txn_id: 1,
                table: "t".into(),
                row_id: 0,
                row: vec![Value::Int(1), Value::Text("x".into())],
            })
            .unwrap();
            w.append(&WalRecord::Commit { txn_id: 1 }).unwrap();
            w.sync().unwrap();
        }
        let mut w = Wal::open(&p).unwrap();
        let recs = w.replay().unwrap();
        assert_eq!(recs.len(), 3);
    }

    #[test]
    fn torn_tail_is_discarded() {
        let d = tempdir().unwrap();
        let p = d.path().join("b.wal");
        {
            let mut w = Wal::open(&p).unwrap();
            w.append(&WalRecord::Begin { txn_id: 1 }).unwrap();
            w.append(&WalRecord::Commit { txn_id: 1 }).unwrap();
            // append a few junk bytes (torn write)
            use std::io::Write as _;
            let mut f = OpenOptions::new().append(true).open(&p).unwrap();
            f.write_all(&[0xAA, 0xBB, 0xCC]).unwrap();
        }
        let mut w = Wal::open(&p).unwrap();
        let recs = w.replay().unwrap();
        assert_eq!(recs.len(), 2);
    }
}

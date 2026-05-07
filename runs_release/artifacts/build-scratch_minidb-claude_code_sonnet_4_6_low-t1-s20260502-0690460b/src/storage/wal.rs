/// Write-Ahead Log (WAL)
///
/// Record format:
///   [4 bytes: record_len as u32 LE]
///   [record_len bytes: record data]
///   [4 bytes: CRC32 of (record_len_bytes || record_data)]
///
/// Record data is a JSON-encoded WalRecord for simplicity.
use crate::error::{MiniDbError, Result};
use crc32fast::Hasher;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

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
        data: Vec<u8>,
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
        old_data: Vec<u8>,
        new_data: Vec<u8>,
    },
    Catalog {
        txn_id: u64,
        data: Vec<u8>,
    },
}

impl WalRecord {
    pub fn txn_id(&self) -> u64 {
        match self {
            WalRecord::Begin { txn_id } => *txn_id,
            WalRecord::Commit { txn_id } => *txn_id,
            WalRecord::Rollback { txn_id } => *txn_id,
            WalRecord::InsertRow { txn_id, .. } => *txn_id,
            WalRecord::DeleteRow { txn_id, .. } => *txn_id,
            WalRecord::UpdateRow { txn_id, .. } => *txn_id,
            WalRecord::Catalog { txn_id, .. } => *txn_id,
        }
    }
}

pub struct Wal {
    writer: BufWriter<File>,
    path: std::path::PathBuf,
}

impl Wal {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Wal {
            writer: BufWriter::new(file),
            path: path.to_path_buf(),
        })
    }

    pub fn append(&mut self, record: &WalRecord) -> Result<()> {
        let data = serde_json::to_vec(record).map_err(|e| MiniDbError::Wal(e.to_string()))?;
        let len = data.len() as u32;
        let len_bytes = len.to_le_bytes();

        let mut hasher = Hasher::new();
        hasher.update(&len_bytes);
        hasher.update(&data);
        let crc = hasher.finalize();

        self.writer.write_all(&len_bytes)?;
        self.writer.write_all(&data)?;
        self.writer.write_all(&crc.to_le_bytes())?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;
        Ok(())
    }

    /// Read all valid records from the WAL file
    pub fn read_all(path: &Path) -> Result<Vec<WalRecord>> {
        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut records = Vec::new();
        let mut buf4 = [0u8; 4];
        loop {
            // Read length
            match file.read_exact(&mut buf4) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
            let len = u32::from_le_bytes(buf4) as usize;
            let mut data = vec![0u8; len];
            match file.read_exact(&mut data) {
                Ok(()) => {}
                Err(_) => {
                    log::warn!("WAL: truncated record, stopping replay");
                    break;
                }
            }
            // Read CRC
            match file.read_exact(&mut buf4) {
                Ok(()) => {}
                Err(_) => {
                    log::warn!("WAL: truncated CRC, stopping replay");
                    break;
                }
            }
            let stored_crc = u32::from_le_bytes(buf4);
            // Verify CRC
            let mut hasher = Hasher::new();
            hasher.update(&(len as u32).to_le_bytes());
            hasher.update(&data);
            let computed_crc = hasher.finalize();
            if computed_crc != stored_crc {
                log::warn!("WAL: CRC mismatch, stopping replay (torn write detected)");
                break;
            }
            match serde_json::from_slice::<WalRecord>(&data) {
                Ok(record) => records.push(record),
                Err(e) => {
                    log::warn!("WAL: failed to deserialize record: {}", e);
                    break;
                }
            }
        }
        Ok(records)
    }

    /// Truncate (checkpoint) the WAL after all committed transactions are applied
    pub fn truncate(&self) -> Result<()> {
        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        drop(file);
        Ok(())
    }
}

/// Replay WAL: returns committed transaction records grouped by txn_id
pub fn replay_committed(records: &[WalRecord]) -> Vec<&WalRecord> {
    // Find committed txn IDs
    let committed: std::collections::HashSet<u64> = records
        .iter()
        .filter_map(|r| {
            if matches!(r, WalRecord::Commit { .. }) {
                Some(r.txn_id())
            } else {
                None
            }
        })
        .collect();
    records
        .iter()
        .filter(|r| committed.contains(&r.txn_id()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_wal_roundtrip() {
        let tmp = NamedTempFile::new().unwrap();
        let mut wal = Wal::open(tmp.path()).unwrap();
        wal.append(&WalRecord::Begin { txn_id: 1 }).unwrap();
        wal.append(&WalRecord::InsertRow {
            txn_id: 1,
            table: "t".into(),
            row_id: 0,
            data: vec![1, 2, 3],
        })
        .unwrap();
        wal.append(&WalRecord::Commit { txn_id: 1 }).unwrap();
        wal.flush().unwrap();

        let records = Wal::read_all(tmp.path()).unwrap();
        assert_eq!(records.len(), 3);
        assert!(matches!(records[0], WalRecord::Begin { txn_id: 1 }));
        assert!(matches!(records[2], WalRecord::Commit { txn_id: 1 }));
    }

    #[test]
    fn test_wal_replay_committed() {
        let tmp = NamedTempFile::new().unwrap();
        let mut wal = Wal::open(tmp.path()).unwrap();
        // Txn 1: committed
        wal.append(&WalRecord::Begin { txn_id: 1 }).unwrap();
        wal.append(&WalRecord::InsertRow {
            txn_id: 1,
            table: "t".into(),
            row_id: 0,
            data: vec![],
        })
        .unwrap();
        wal.append(&WalRecord::Commit { txn_id: 1 }).unwrap();
        // Txn 2: uncommitted
        wal.append(&WalRecord::Begin { txn_id: 2 }).unwrap();
        wal.append(&WalRecord::InsertRow {
            txn_id: 2,
            table: "t".into(),
            row_id: 1,
            data: vec![],
        })
        .unwrap();
        wal.flush().unwrap();

        let records = Wal::read_all(tmp.path()).unwrap();
        let committed = replay_committed(&records);
        assert_eq!(committed.len(), 3); // Begin, Insert, Commit for txn 1
        for r in &committed {
            assert_eq!(r.txn_id(), 1);
        }
    }
}

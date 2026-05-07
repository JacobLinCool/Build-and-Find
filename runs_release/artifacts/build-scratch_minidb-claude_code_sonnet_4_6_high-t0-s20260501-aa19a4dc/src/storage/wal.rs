/// Write-Ahead Log: append-only, record-oriented, with CRC32 per record.
///
/// Record format:
///   [4 bytes: payload len][4 bytes: CRC32 of payload][N bytes: payload]
///
/// WAL records are replayed on startup to recover committed transactions.
use crate::error::{MiniDbError, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use crc32fast::Hasher as Crc32Hasher;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// A single WAL record type
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
    UpdateRow {
        txn_id: u64,
        table: String,
        row_id: u64,
        old_data: Vec<u8>,
        new_data: Vec<u8>,
    },
    DeleteRow {
        txn_id: u64,
        table: String,
        row_id: u64,
        old_data: Vec<u8>,
    },
    CatalogChange {
        txn_id: u64,
        payload: Vec<u8>,
    },
    IndexInsert {
        txn_id: u64,
        index: String,
        key_data: Vec<u8>,
        row_id: u64,
    },
    IndexDelete {
        txn_id: u64,
        index: String,
        key_data: Vec<u8>,
        row_id: u64,
    },
    Checkpoint,
}

impl WalRecord {
    pub fn txn_id(&self) -> Option<u64> {
        match self {
            WalRecord::Begin { txn_id }
            | WalRecord::Commit { txn_id }
            | WalRecord::Rollback { txn_id }
            | WalRecord::InsertRow { txn_id, .. }
            | WalRecord::UpdateRow { txn_id, .. }
            | WalRecord::DeleteRow { txn_id, .. }
            | WalRecord::CatalogChange { txn_id, .. }
            | WalRecord::IndexInsert { txn_id, .. }
            | WalRecord::IndexDelete { txn_id, .. } => Some(*txn_id),
            WalRecord::Checkpoint => None,
        }
    }
}

pub struct WalWriter {
    writer: BufWriter<File>,
}

impl WalWriter {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = OpenOptions::new().append(true).create(true).open(path)?;
        Ok(WalWriter {
            writer: BufWriter::new(file),
        })
    }

    pub fn write_record(&mut self, record: &WalRecord) -> Result<()> {
        let payload = serde_json::to_vec(record).map_err(|e| MiniDbError::Wal(e.to_string()))?;

        let crc = compute_crc32(&payload);

        self.writer
            .write_u32::<LittleEndian>(payload.len() as u32)?;
        self.writer.write_u32::<LittleEndian>(crc)?;
        self.writer.write_all(&payload)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;
        Ok(())
    }
}

pub struct WalReader {
    reader: BufReader<File>,
}

impl WalReader {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = OpenOptions::new().read(true).open(path)?;
        Ok(WalReader {
            reader: BufReader::new(file),
        })
    }

    pub fn read_record(&mut self) -> Result<Option<WalRecord>> {
        let len = match self.reader.read_u32::<LittleEndian>() {
            Ok(l) => l,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let crc = self.reader.read_u32::<LittleEndian>()?;

        let mut payload = vec![0u8; len as usize];
        self.reader.read_exact(&mut payload)?;

        let computed_crc = compute_crc32(&payload);
        if computed_crc != crc {
            return Err(MiniDbError::CrcMismatch);
        }

        let record: WalRecord = serde_json::from_slice(&payload)
            .map_err(|e| MiniDbError::Wal(format!("deserialization error: {}", e)))?;

        Ok(Some(record))
    }

    pub fn read_all(&mut self) -> Result<Vec<WalRecord>> {
        let mut records = Vec::new();
        loop {
            match self.read_record() {
                Ok(Some(r)) => records.push(r),
                Ok(None) => break,
                Err(MiniDbError::CrcMismatch) => {
                    // Torn write at the tail — stop here (partial record)
                    log::warn!("WAL: CRC mismatch at tail, treating as torn write");
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(records)
    }
}

/// Replay WAL records to find committed transactions.
/// Returns only records belonging to committed transactions,
/// in order, excluding Begin/Commit/Rollback control records.
pub fn collect_committed_ops(records: &[WalRecord]) -> Vec<&WalRecord> {
    use std::collections::HashSet;

    let mut committed_txns: HashSet<u64> = HashSet::new();
    let mut rolled_back: HashSet<u64> = HashSet::new();

    for record in records {
        match record {
            WalRecord::Commit { txn_id } => {
                committed_txns.insert(*txn_id);
            }
            WalRecord::Rollback { txn_id } => {
                rolled_back.insert(*txn_id);
            }
            _ => {}
        }
    }

    records
        .iter()
        .filter(|r| {
            if let Some(tid) = r.txn_id() {
                committed_txns.contains(&tid)
                    && !matches!(
                        r,
                        WalRecord::Begin { .. }
                            | WalRecord::Commit { .. }
                            | WalRecord::Rollback { .. }
                    )
            } else {
                false
            }
        })
        .collect()
}

fn compute_crc32(data: &[u8]) -> u32 {
    let mut hasher = Crc32Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

/// Manage both reading and writing the WAL, with truncation support.
pub struct Wal {
    path: std::path::PathBuf,
    writer: WalWriter,
}

impl Wal {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let writer = WalWriter::open(&path)?;
        Ok(Wal { path, writer })
    }

    pub fn append(&mut self, record: &WalRecord) -> Result<()> {
        self.writer.write_record(record)
    }

    pub fn sync(&mut self) -> Result<()> {
        self.writer.sync()
    }

    pub fn replay(&self) -> Result<Vec<WalRecord>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let mut reader = WalReader::open(&self.path)?;
        reader.read_all()
    }

    /// Truncate the WAL after a checkpoint (keep the file but reset it)
    pub fn truncate(&mut self) -> Result<()> {
        // Drop current writer, truncate file, reopen
        let path = self.path.clone();
        let file = OpenOptions::new().write(true).truncate(true).open(&path)?;
        drop(file);
        self.writer = WalWriter::open(&path)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_write_and_read_records() {
        let tmp = NamedTempFile::new().unwrap();
        let mut writer = WalWriter::open(tmp.path()).unwrap();

        writer
            .write_record(&WalRecord::Begin { txn_id: 1 })
            .unwrap();
        writer
            .write_record(&WalRecord::InsertRow {
                txn_id: 1,
                table: "users".into(),
                row_id: 1,
                data: vec![1, 2, 3],
            })
            .unwrap();
        writer
            .write_record(&WalRecord::Commit { txn_id: 1 })
            .unwrap();
        writer.sync().unwrap();

        let mut reader = WalReader::open(tmp.path()).unwrap();
        let records = reader.read_all().unwrap();
        assert_eq!(records.len(), 3);
    }

    #[test]
    fn test_crc_mismatch_on_corruption() {
        let tmp = NamedTempFile::new().unwrap();
        {
            let mut writer = WalWriter::open(tmp.path()).unwrap();
            writer
                .write_record(&WalRecord::Begin { txn_id: 1 })
                .unwrap();
            writer.sync().unwrap();
        }

        // Corrupt the CRC
        let mut file = OpenOptions::new().write(true).open(tmp.path()).unwrap();
        file.seek(SeekFrom::Start(4)).unwrap();
        file.write_all(&[0xFF, 0xFF, 0xFF, 0xFF]).unwrap();
        drop(file);

        let mut reader = WalReader::open(tmp.path()).unwrap();
        let records = reader.read_all().unwrap();
        // Corruption detected, treated as torn write
        assert_eq!(records.len(), 0);
    }

    #[test]
    fn test_collect_committed_ops() {
        let records = vec![
            WalRecord::Begin { txn_id: 1 },
            WalRecord::InsertRow {
                txn_id: 1,
                table: "t".into(),
                row_id: 1,
                data: vec![],
            },
            WalRecord::Commit { txn_id: 1 },
            WalRecord::Begin { txn_id: 2 },
            WalRecord::InsertRow {
                txn_id: 2,
                table: "t".into(),
                row_id: 2,
                data: vec![],
            },
            WalRecord::Rollback { txn_id: 2 },
        ];

        let ops = collect_committed_ops(&records);
        assert_eq!(ops.len(), 1);
    }
}

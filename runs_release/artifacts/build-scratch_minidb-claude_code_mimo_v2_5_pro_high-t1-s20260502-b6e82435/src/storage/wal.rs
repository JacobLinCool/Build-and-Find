use crate::error::{MiniDbError, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// WAL record types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalOp {
    /// A page was written: (page_id, data)
    PageWrite = 1,
    /// Transaction commit marker
    Commit = 2,
    /// Transaction begin marker
    Begin = 3,
    /// Catalog update: serialized catalog data
    CatalogUpdate = 4,
    /// Checkpoint: all prior data flushed
    Checkpoint = 5,
}

impl WalOp {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(WalOp::PageWrite),
            2 => Some(WalOp::Commit),
            3 => Some(WalOp::Begin),
            4 => Some(WalOp::CatalogUpdate),
            5 => Some(WalOp::Checkpoint),
            _ => None,
        }
    }
}

/// A single WAL record.
///
/// Format:
///   [4 bytes: CRC32 of everything after this field]
///   [1 byte: op type]
///   [8 bytes: txn_id]
///   [4 bytes: data length]
///   [N bytes: data]
#[derive(Debug, Clone)]
pub struct WalRecord {
    pub op: WalOp,
    pub txn_id: u64,
    pub data: Vec<u8>,
}

impl WalRecord {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.push(self.op as u8);
        body.extend_from_slice(&self.txn_id.to_le_bytes());
        body.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        body.extend_from_slice(&self.data);

        let crc = crc32fast::hash(&body);
        let mut record = Vec::with_capacity(4 + body.len());
        record.extend_from_slice(&crc.to_le_bytes());
        record.extend_from_slice(&body);
        record
    }

    pub fn decode(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() < 17 {
            return Err(MiniDbError::Wal("WAL record too short".into()));
        }

        let crc = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let body = &data[4..];

        // Read data_len first to know the actual record body size
        let op = WalOp::from_u8(body[0])
            .ok_or_else(|| MiniDbError::Wal(format!("unknown WAL op: {}", body[0])))?;
        let txn_id = u64::from_le_bytes(body[1..9].try_into().unwrap());
        let data_len = u32::from_le_bytes(body[9..13].try_into().unwrap()) as usize;

        if body.len() < 13 + data_len {
            return Err(MiniDbError::Wal("WAL record data truncated".into()));
        }

        // CRC is computed only over the record body (not the rest of the file)
        let record_body = &body[..13 + data_len];
        let computed_crc = crc32fast::hash(record_body);
        if crc != computed_crc {
            return Err(MiniDbError::Wal(
                "WAL record CRC mismatch (torn write)".into(),
            ));
        }

        let record_data = body[13..13 + data_len].to_vec();
        let total_len = 4 + 13 + data_len;

        Ok((
            WalRecord {
                op,
                txn_id,
                data: record_data,
            },
            total_len,
        ))
    }
}

pub struct Wal {
    file: File,
    path: String,
    offset: u64,
    current_txn_id: u64,
}

impl Wal {
    pub fn open(path: &str) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let offset = file.metadata()?.len();

        Ok(Self {
            file,
            path: path.to_string(),
            offset,
            current_txn_id: 0,
        })
    }

    pub fn create(path: &str) -> Result<Self> {
        if Path::new(path).exists() {
            std::fs::remove_file(path)?;
        }
        Self::open(path)
    }

    pub fn next_txn_id(&mut self) -> u64 {
        self.current_txn_id += 1;
        self.current_txn_id
    }

    pub fn append(&mut self, record: &WalRecord) -> Result<()> {
        let encoded = record.encode();
        self.file.seek(SeekFrom::Start(self.offset))?;
        self.file.write_all(&encoded)?;
        self.file.flush()?;
        self.file.sync_data()?;
        self.offset += encoded.len() as u64;
        Ok(())
    }

    pub fn append_page_write(&mut self, txn_id: u64, page_id: u64, data: &[u8]) -> Result<()> {
        let mut record_data = Vec::with_capacity(8 + data.len());
        record_data.extend_from_slice(&page_id.to_le_bytes());
        record_data.extend_from_slice(data);

        self.append(&WalRecord {
            op: WalOp::PageWrite,
            txn_id,
            data: record_data,
        })
    }

    pub fn append_commit(&mut self, txn_id: u64) -> Result<()> {
        self.append(&WalRecord {
            op: WalOp::Commit,
            txn_id,
            data: Vec::new(),
        })
    }

    pub fn append_begin(&mut self, txn_id: u64) -> Result<()> {
        self.append(&WalRecord {
            op: WalOp::Begin,
            txn_id,
            data: Vec::new(),
        })
    }

    pub fn append_catalog_update(&mut self, txn_id: u64, catalog_data: &[u8]) -> Result<()> {
        self.append(&WalRecord {
            op: WalOp::CatalogUpdate,
            txn_id,
            data: catalog_data.to_vec(),
        })
    }

    pub fn append_checkpoint(&mut self) -> Result<()> {
        self.append(&WalRecord {
            op: WalOp::Checkpoint,
            txn_id: 0,
            data: Vec::new(),
        })
    }

    /// Read all WAL records for replay.
    pub fn read_all(&mut self) -> Result<Vec<WalRecord>> {
        let buf = Self::read_file(&self.path)?;
        Self::decode_records(&buf)
    }

    fn read_file(path: &str) -> Result<Vec<u8>> {
        use std::io::Read;
        let mut f = std::fs::File::open(path)
            .map_err(|e| MiniDbError::Wal(format!("failed to open WAL '{}': {}", path, e)))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| MiniDbError::Wal(format!("failed to read WAL '{}': {}", path, e)))?;
        Ok(buf)
    }

    fn decode_records(buf: &[u8]) -> Result<Vec<WalRecord>> {
        let mut records = Vec::new();
        let mut pos = 0;

        while pos < buf.len() {
            if pos + 17 > buf.len() {
                break; // incomplete record
            }
            match WalRecord::decode(&buf[pos..]) {
                Ok((record, len)) => {
                    records.push(record);
                    pos += len;
                }
                Err(e) => {
                    log::warn!("WAL record decode error at offset {}: {}", pos, e);
                    break; // corrupted record, stop replay
                }
            }
        }

        Ok(records)
    }

    /// Replay the WAL: returns committed page writes and catalog data.
    pub fn replay(&mut self) -> Result<WalReplayResult> {
        let records = self.read_all()?;
        let mut result = WalReplayResult::default();
        let mut committed_txns = std::collections::HashSet::new();
        let mut pending_writes: Vec<(u64, u64, Vec<u8>)> = Vec::new();
        let mut pending_catalog: Option<Vec<u8>> = None;
        let _last_checkpoint_pos = 0;

        // First pass: find committed transactions
        for record in &records {
            match record.op {
                WalOp::Commit => {
                    committed_txns.insert(record.txn_id);
                }
                WalOp::Checkpoint => {
                    // Everything before checkpoint is already flushed
                    committed_txns.clear();
                    pending_writes.clear();
                    pending_catalog = None;
                }
                _ => {}
            }
        }

        // Second pass: collect writes from committed transactions
        for record in &records {
            match record.op {
                WalOp::PageWrite => {
                    if committed_txns.contains(&record.txn_id) {
                        let page_id = u64::from_le_bytes(record.data[0..8].try_into().unwrap());
                        let page_data = record.data[8..].to_vec();
                        pending_writes.push((record.txn_id, page_id, page_data));
                    }
                }
                WalOp::CatalogUpdate => {
                    if committed_txns.contains(&record.txn_id) {
                        pending_catalog = Some(record.data.clone());
                    }
                }
                WalOp::Checkpoint => {
                    pending_writes.clear();
                    pending_catalog = None;
                }
                _ => {}
            }
        }

        result.page_writes = pending_writes;
        result.catalog_data = pending_catalog;
        result.has_uncommitted = !committed_txns.is_empty()
            && records.last().map_or(false, |r| {
                r.op != WalOp::Commit && r.op != WalOp::Checkpoint
            });

        Ok(result)
    }

    pub fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.offset = 0;
        Ok(())
    }

    pub fn sync(&mut self) -> Result<()> {
        self.file.flush()?;
        self.file.sync_data()?;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct WalReplayResult {
    pub page_writes: Vec<(u64, u64, Vec<u8>)>,
    pub catalog_data: Option<Vec<u8>>,
    pub has_uncommitted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_wal_record_encode_decode() {
        let record = WalRecord {
            op: WalOp::PageWrite,
            txn_id: 42,
            data: vec![1, 2, 3, 4, 5],
        };

        let encoded = record.encode();
        let (decoded, len) = WalRecord::decode(&encoded).unwrap();

        assert_eq!(len, encoded.len());
        assert_eq!(decoded.op, WalOp::PageWrite);
        assert_eq!(decoded.txn_id, 42);
        assert_eq!(decoded.data, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_wal_record_crc_detection() {
        let record = WalRecord {
            op: WalOp::Commit,
            txn_id: 1,
            data: Vec::new(),
        };

        let mut encoded = record.encode();
        // Corrupt a byte
        encoded[5] ^= 0xFF;
        assert!(WalRecord::decode(&encoded).is_err());
    }

    #[test]
    fn test_wal_append_and_replay() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");
        let path_str = path.to_str().unwrap();

        {
            let mut wal = Wal::create(path_str).unwrap();
            let txn_id = wal.next_txn_id();
            wal.append_begin(txn_id).unwrap();
            wal.append_page_write(txn_id, 0, &[42u8; 4096]).unwrap();
            wal.append_commit(txn_id).unwrap();
        }

        let mut wal = Wal::open(path_str).unwrap();
        let result = wal.replay().unwrap();
        assert_eq!(result.page_writes.len(), 1);
        assert_eq!(result.page_writes[0].1, 0); // page_id
    }

    #[test]
    fn test_wal_uncommitted_discard() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");
        let path_str = path.to_str().unwrap();

        {
            let mut wal = Wal::create(path_str).unwrap();
            // Committed transaction
            let txn1 = wal.next_txn_id();
            wal.append_begin(txn1).unwrap();
            wal.append_page_write(txn1, 0, &[1u8; 100]).unwrap();
            wal.append_commit(txn1).unwrap();

            // Uncommitted transaction
            let txn2 = wal.next_txn_id();
            wal.append_begin(txn2).unwrap();
            wal.append_page_write(txn2, 1, &[2u8; 100]).unwrap();
            // No commit!
        }

        let mut wal = Wal::open(path_str).unwrap();
        let result = wal.replay().unwrap();
        // Only the committed write should be replayed
        assert_eq!(result.page_writes.len(), 1);
        assert_eq!(result.page_writes[0].0, 1); // txn_id
    }
}

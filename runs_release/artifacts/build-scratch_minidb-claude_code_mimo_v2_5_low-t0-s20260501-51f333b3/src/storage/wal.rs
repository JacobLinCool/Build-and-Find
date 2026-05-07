use crate::error::{MiniDbError, Result};
use crate::storage::page::PAGE_SIZE;
use log::debug;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum WalRecord {
    BeginTxn {
        txn_id: u64,
    },
    Write {
        txn_id: u64,
        page_id: u64,
        offset: u16,
        data: Vec<u8>,
    },
    PageCreate {
        txn_id: u64,
        page_id: u64,
    },
    CommitTxn {
        txn_id: u64,
    },
}

impl WalRecord {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            WalRecord::BeginTxn { txn_id } => {
                buf.push(1); // type
                buf.extend_from_slice(&txn_id.to_le_bytes());
            }
            WalRecord::Write {
                txn_id,
                page_id,
                offset,
                data,
            } => {
                buf.push(2); // type
                buf.extend_from_slice(&txn_id.to_le_bytes());
                buf.extend_from_slice(&page_id.to_le_bytes());
                buf.extend_from_slice(&offset.to_le_bytes());
                buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
                buf.extend_from_slice(data);
            }
            WalRecord::PageCreate { txn_id, page_id } => {
                buf.push(3); // type
                buf.extend_from_slice(&txn_id.to_le_bytes());
                buf.extend_from_slice(&page_id.to_le_bytes());
            }
            WalRecord::CommitTxn { txn_id } => {
                buf.push(4); // type
                buf.extend_from_slice(&txn_id.to_le_bytes());
            }
        }

        // Add CRC32
        let crc = crc32fast::hash(&buf);
        let mut result = buf;
        result.extend_from_slice(&crc.to_le_bytes());
        result
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 5 {
            return Err(MiniDbError::StorageError(
                "WAL record too short".to_string(),
            ));
        }

        // Verify CRC
        let payload = &data[..data.len() - 4];
        let stored_crc = u32::from_le_bytes([
            data[data.len() - 4],
            data[data.len() - 3],
            data[data.len() - 2],
            data[data.len() - 1],
        ]);
        let computed_crc = crc32fast::hash(payload);
        if stored_crc != computed_crc {
            return Err(MiniDbError::StorageError(
                "WAL record CRC mismatch (torn write)".to_string(),
            ));
        }

        match data[0] {
            1 => {
                let txn_id = u64::from_le_bytes([
                    data[1], data[2], data[3], data[4], data[5], data[6], data[7], data[8],
                ]);
                Ok(WalRecord::BeginTxn { txn_id })
            }
            2 => {
                let txn_id = u64::from_le_bytes([
                    data[1], data[2], data[3], data[4], data[5], data[6], data[7], data[8],
                ]);
                let page_id = u64::from_le_bytes([
                    data[9], data[10], data[11], data[12], data[13], data[14], data[15], data[16],
                ]);
                let offset = u16::from_le_bytes([data[17], data[18]]);
                let len = u32::from_le_bytes([data[19], data[20], data[21], data[22]]) as usize;
                let mut page_data = vec![0u8; len];
                page_data.copy_from_slice(&data[23..23 + len]);
                Ok(WalRecord::Write {
                    txn_id,
                    page_id,
                    offset,
                    data: page_data,
                })
            }
            3 => {
                let txn_id = u64::from_le_bytes([
                    data[1], data[2], data[3], data[4], data[5], data[6], data[7], data[8],
                ]);
                let page_id = u64::from_le_bytes([
                    data[9], data[10], data[11], data[12], data[13], data[14], data[15], data[16],
                ]);
                Ok(WalRecord::PageCreate { txn_id, page_id })
            }
            4 => {
                let txn_id = u64::from_le_bytes([
                    data[1], data[2], data[3], data[4], data[5], data[6], data[7], data[8],
                ]);
                Ok(WalRecord::CommitTxn { txn_id })
            }
            t => Err(MiniDbError::StorageError(format!(
                "unknown WAL record type: {}",
                t
            ))),
        }
    }
}

pub struct WalWriter {
    file: File,
}

impl WalWriter {
    pub fn new(path: &Path) -> Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(WalWriter { file })
    }

    pub fn append(&mut self, record: &WalRecord) -> Result<()> {
        let encoded = record.encode();
        let len = encoded.len() as u32;
        self.file.write_all(&len.to_le_bytes())?;
        self.file.write_all(&encoded)?;
        self.file.sync_all()?;
        debug!(
            "WAL: appended record {:?} ({} bytes)",
            record,
            encoded.len()
        );
        Ok(())
    }

    pub fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        Ok(())
    }
}

pub struct WalReader {
    file: File,
}

impl WalReader {
    pub fn new(path: &Path) -> Result<Self> {
        let file = OpenOptions::new().read(true).open(path)?;
        Ok(WalReader { file })
    }

    pub fn read_all(&mut self) -> Result<Vec<WalRecord>> {
        let mut records = Vec::new();
        loop {
            let mut len_buf = [0u8; 4];
            match self.file.read_exact(&mut len_buf) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
            let len = u32::from_le_bytes(len_buf) as usize;
            if len > PAGE_SIZE * 2 {
                return Err(MiniDbError::StorageError(
                    "WAL record too large".to_string(),
                ));
            }
            let mut data = vec![0u8; len];
            self.file.read_exact(&mut data)?;
            let record = WalRecord::decode(&data)?;
            records.push(record);
        }
        Ok(records)
    }
}

/// Replay WAL to recover committed-but-unflushed transactions
pub fn replay_wal(wal_path: &Path) -> Result<Vec<WalRecord>> {
    if !wal_path.exists() {
        return Ok(Vec::new());
    }

    let mut reader = WalReader::new(wal_path)?;
    let records = reader.read_all()?;

    // Find committed transactions
    let mut committed = std::collections::HashSet::new();
    for record in &records {
        if let WalRecord::CommitTxn { txn_id } = record {
            committed.insert(*txn_id);
        }
    }

    // Only return records from committed transactions
    let replay_records: Vec<WalRecord> = records
        .into_iter()
        .filter(|r| {
            let txn_id = match r {
                WalRecord::BeginTxn { txn_id } => *txn_id,
                WalRecord::Write { txn_id, .. } => *txn_id,
                WalRecord::PageCreate { txn_id, .. } => *txn_id,
                WalRecord::CommitTxn { txn_id } => *txn_id,
            };
            committed.contains(&txn_id)
        })
        .collect();

    debug!("WAL replay: {} records to replay", replay_records.len());
    Ok(replay_records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_wal_record_encode_decode() {
        let record = WalRecord::BeginTxn { txn_id: 1 };
        let encoded = record.encode();
        let decoded = WalRecord::decode(&encoded).unwrap();
        assert_eq!(record, decoded);

        let record = WalRecord::Write {
            txn_id: 1,
            page_id: 42,
            offset: 100,
            data: vec![1, 2, 3, 4],
        };
        let encoded = record.encode();
        let decoded = WalRecord::decode(&encoded).unwrap();
        assert_eq!(record, decoded);
    }

    #[test]
    fn test_wal_torn_write_detection() {
        let record = WalRecord::BeginTxn { txn_id: 1 };
        let mut encoded = record.encode();
        // Corrupt the data
        encoded[1] = 0xFF;
        assert!(WalRecord::decode(&encoded).is_err());
    }

    #[test]
    fn test_wal_read_write() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        {
            let mut writer = WalWriter::new(&wal_path).unwrap();
            writer.append(&WalRecord::BeginTxn { txn_id: 1 }).unwrap();
            writer
                .append(&WalRecord::Write {
                    txn_id: 1,
                    page_id: 0,
                    offset: 0,
                    data: vec![1, 2, 3],
                })
                .unwrap();
            writer.append(&WalRecord::CommitTxn { txn_id: 1 }).unwrap();
        }

        let mut reader = WalReader::new(&wal_path).unwrap();
        let records = reader.read_all().unwrap();
        assert_eq!(records.len(), 3);
    }
}

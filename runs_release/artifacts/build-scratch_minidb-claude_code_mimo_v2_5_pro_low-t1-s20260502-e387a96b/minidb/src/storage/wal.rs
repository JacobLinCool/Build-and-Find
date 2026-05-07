use crate::error::{MiniDbError, Result};
use crate::storage::PAGE_SIZE;
use crc32fast::Hasher;
use log::debug;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

const RECORD_BEGIN: u8 = 1;
const RECORD_PAGE_WRITE: u8 = 2;
const RECORD_COMMIT: u8 = 3;

#[derive(Debug, Clone)]
pub enum WalRecord {
    Begin {
        txn_id: u64,
    },
    PageWrite {
        table_id: u64,
        page_id: u64,
        data: [u8; PAGE_SIZE],
    },
    Commit {
        txn_id: u64,
    },
}

impl WalRecord {
    fn type_byte(&self) -> u8 {
        match self {
            Self::Begin { .. } => RECORD_BEGIN,
            Self::PageWrite { .. } => RECORD_PAGE_WRITE,
            Self::Commit { .. } => RECORD_COMMIT,
        }
    }

    fn encode_payload(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::Begin { txn_id } => {
                buf.extend_from_slice(&txn_id.to_le_bytes());
            }
            Self::PageWrite {
                table_id,
                page_id,
                data,
            } => {
                buf.extend_from_slice(&table_id.to_le_bytes());
                buf.extend_from_slice(&page_id.to_le_bytes());
                buf.extend_from_slice(data);
            }
            Self::Commit { txn_id } => {
                buf.extend_from_slice(&txn_id.to_le_bytes());
            }
        }
        buf
    }

    fn decode(type_byte: u8, data: &[u8]) -> Result<Self> {
        match type_byte {
            RECORD_BEGIN => {
                if data.len() < 8 {
                    return Err(MiniDbError::WalCorruption("truncated begin record".into()));
                }
                let txn_id = u64::from_le_bytes(data[0..8].try_into().unwrap());
                Ok(Self::Begin { txn_id })
            }
            RECORD_PAGE_WRITE => {
                let min_size = 8 + 8 + PAGE_SIZE;
                if data.len() < min_size {
                    return Err(MiniDbError::WalCorruption(
                        "truncated page write record".into(),
                    ));
                }
                let table_id = u64::from_le_bytes(data[0..8].try_into().unwrap());
                let page_id = u64::from_le_bytes(data[8..16].try_into().unwrap());
                let mut page_data = [0u8; PAGE_SIZE];
                page_data.copy_from_slice(&data[16..16 + PAGE_SIZE]);
                Ok(Self::PageWrite {
                    table_id,
                    page_id,
                    data: page_data,
                })
            }
            RECORD_COMMIT => {
                if data.len() < 8 {
                    return Err(MiniDbError::WalCorruption("truncated commit record".into()));
                }
                let txn_id = u64::from_le_bytes(data[0..8].try_into().unwrap());
                Ok(Self::Commit { txn_id })
            }
            other => Err(MiniDbError::WalCorruption(format!(
                "unknown record type: {other}"
            ))),
        }
    }

    /// Encoded size: 4 (crc) + 4 (len) + 1 (type) + payload
    fn total_size(&self) -> usize {
        4 + 4 + 1 + self.encode_payload().len()
    }
}

pub struct Wal {
    path: std::path::PathBuf,
    writer: BufWriter<File>,
    offset: u64,
}

impl Wal {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let end = file.seek(SeekFrom::End(0))?;
        Ok(Self {
            path: path.to_path_buf(),
            writer: BufWriter::new(file),
            offset: end,
        })
    }

    pub fn append(&mut self, record: &WalRecord) -> Result<()> {
        let payload = record.encode_payload();
        let len = (1 + payload.len()) as u32;

        let mut hasher = Hasher::new();
        hasher.update(&[record.type_byte()]);
        hasher.update(&payload);
        let crc = hasher.finalize();

        self.writer.write_all(&crc.to_le_bytes())?;
        self.writer.write_all(&len.to_le_bytes())?;
        self.writer.write_all(&[record.type_byte()])?;
        self.writer.write_all(&payload)?;
        self.offset += 4 + 4 + 1 + payload.len() as u64;
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

    /// Replay WAL, returning committed page writes grouped by txn_id.
    pub fn replay(path: &Path) -> Result<Vec<WalRecord>> {
        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };

        let mut all_records = Vec::new();
        loop {
            let mut hdr = [0u8; 8];
            match file.read_exact(&mut hdr) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }

            let crc = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
            let len = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;

            let mut body = vec![0u8; len];
            match file.read_exact(&mut body) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    debug!("WAL: truncated record at end, stopping replay");
                    break;
                }
                Err(e) => return Err(e.into()),
            }

            let mut hasher = Hasher::new();
            hasher.update(&body);
            if hasher.finalize() != crc {
                debug!("WAL: CRC mismatch, stopping replay");
                break;
            }

            let type_byte = body[0];
            let record = WalRecord::decode(type_byte, &body[1..])?;
            all_records.push(record);
        }

        // Find committed transactions
        let committed_ids: std::collections::HashSet<u64> = all_records
            .iter()
            .filter_map(|r| match r {
                WalRecord::Commit { txn_id } => Some(*txn_id),
                _ => None,
            })
            .collect();

        // Collect committed page writes
        let mut result = Vec::new();
        let mut current_txn: Option<u64> = None;
        for record in &all_records {
            match record {
                WalRecord::Begin { txn_id } => {
                    current_txn = Some(*txn_id);
                }
                WalRecord::PageWrite { .. } => {
                    if let Some(tid) = current_txn {
                        if committed_ids.contains(&tid) {
                            result.push(record.clone());
                        }
                    }
                }
                WalRecord::Commit { txn_id: _ } => {
                    current_txn = None;
                }
            }
        }

        Ok(result)
    }

    /// Truncate WAL (after checkpoint).
    pub fn truncate(path: &Path) -> Result<()> {
        OpenOptions::new().write(true).truncate(true).open(path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_wal_append_and_replay() {
        let path = std::env::temp_dir().join("minidb_wal_test.wal");
        let _ = fs::remove_file(&path);
        {
            let mut wal = Wal::open(&path).unwrap();
            wal.append(&WalRecord::Begin { txn_id: 1 }).unwrap();
            let mut data = [0u8; PAGE_SIZE];
            data[0] = 42;
            wal.append(&WalRecord::PageWrite {
                table_id: 0,
                page_id: 0,
                data,
            })
            .unwrap();
            wal.append(&WalRecord::Commit { txn_id: 1 }).unwrap();
            wal.sync().unwrap();
        }
        {
            let records = Wal::replay(&path).unwrap();
            assert_eq!(records.len(), 1);
            match &records[0] {
                WalRecord::PageWrite { data, .. } => assert_eq!(data[0], 42),
                _ => panic!("expected page write"),
            }
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_wal_discards_uncommitted() {
        let path = std::env::temp_dir().join("minidb_wal_uncommitted.wal");
        let _ = fs::remove_file(&path);
        {
            let mut wal = Wal::open(&path).unwrap();
            // Committed txn
            wal.append(&WalRecord::Begin { txn_id: 1 }).unwrap();
            let mut d1 = [0u8; PAGE_SIZE];
            d1[0] = 1;
            wal.append(&WalRecord::PageWrite {
                table_id: 0,
                page_id: 0,
                data: d1,
            })
            .unwrap();
            wal.append(&WalRecord::Commit { txn_id: 1 }).unwrap();
            // Uncommitted txn
            wal.append(&WalRecord::Begin { txn_id: 2 }).unwrap();
            let mut d2 = [0u8; PAGE_SIZE];
            d2[0] = 2;
            wal.append(&WalRecord::PageWrite {
                table_id: 0,
                page_id: 0,
                data: d2,
            })
            .unwrap();
            // No commit for txn 2
            wal.sync().unwrap();
        }
        {
            let records = Wal::replay(&path).unwrap();
            assert_eq!(records.len(), 1);
            match &records[0] {
                WalRecord::PageWrite { data, .. } => assert_eq!(data[0], 1),
                _ => panic!("expected page write"),
            }
        }
        let _ = fs::remove_file(&path);
    }
}

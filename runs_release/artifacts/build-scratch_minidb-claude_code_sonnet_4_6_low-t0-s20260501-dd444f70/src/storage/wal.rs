use crate::error::{MiniDbError, Result};
use crc32fast::Hasher;
use log::{debug, warn};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WalRecordType {
    Begin = 1,
    Write = 2,
    Commit = 3,
    Rollback = 4,
    Checkpoint = 5,
}

impl WalRecordType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Begin),
            2 => Some(Self::Write),
            3 => Some(Self::Commit),
            4 => Some(Self::Rollback),
            5 => Some(Self::Checkpoint),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WalRecord {
    pub txn_id: u64,
    pub record_type: WalRecordType,
    pub page_id: u64,
    pub data: Vec<u8>,
}

// WAL record on disk:
// [total_len: u32][record_type: u8][txn_id: u64][page_id: u64][data_len: u32][data...][crc32: u32]
// total_len includes everything except total_len itself

const WAL_RECORD_HEADER_SIZE: usize = 1 + 8 + 8 + 4; // type + txn_id + page_id + data_len
const WAL_CRC_SIZE: usize = 4;

pub struct Wal {
    file: BufWriter<File>,
    pub path: std::path::PathBuf,
}

impl Wal {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path.as_ref())?;
        Ok(Wal {
            file: BufWriter::new(file),
            path: path.as_ref().to_path_buf(),
        })
    }

    pub fn append(&mut self, record: &WalRecord) -> Result<()> {
        let data_len = record.data.len() as u32;
        let body_len = WAL_RECORD_HEADER_SIZE + record.data.len() + WAL_CRC_SIZE;

        let mut buf = Vec::with_capacity(4 + body_len);
        buf.extend_from_slice(&(body_len as u32).to_le_bytes());
        buf.push(record.record_type as u8);
        buf.extend_from_slice(&record.txn_id.to_le_bytes());
        buf.extend_from_slice(&record.page_id.to_le_bytes());
        buf.extend_from_slice(&data_len.to_le_bytes());
        buf.extend_from_slice(&record.data);

        let mut hasher = Hasher::new();
        hasher.update(&buf[4..]);
        let crc = hasher.finalize();
        buf.extend_from_slice(&crc.to_le_bytes());

        self.file.write_all(&buf)?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.file.flush()?;
        self.file.get_mut().sync_all()?;
        Ok(())
    }

    pub fn replay<P: AsRef<Path>>(path: P) -> Result<Vec<WalRecord>> {
        let mut file = match File::open(path.as_ref()) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };

        file.seek(SeekFrom::Start(0))?;
        let mut records = Vec::new();
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        let mut pos = 0;

        while pos + 4 <= buf.len() {
            let body_len = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;

            if pos + body_len > buf.len() {
                warn!("WAL: truncated record at offset {}", pos - 4);
                break;
            }

            let record_bytes = &buf[pos..pos + body_len];
            let data_portion = &record_bytes[..body_len - WAL_CRC_SIZE];
            let stored_crc =
                u32::from_le_bytes(record_bytes[body_len - WAL_CRC_SIZE..].try_into().unwrap());

            let mut hasher = Hasher::new();
            hasher.update(data_portion);
            let computed_crc = hasher.finalize();

            if stored_crc != computed_crc {
                warn!(
                    "WAL: CRC mismatch at offset {} (stored={}, computed={}), stopping replay",
                    pos - 4,
                    stored_crc,
                    computed_crc
                );
                break;
            }

            if data_portion.len() < WAL_RECORD_HEADER_SIZE {
                warn!("WAL: record too short at offset {}", pos - 4);
                break;
            }

            let rec_type = WalRecordType::from_u8(data_portion[0]);
            let txn_id = u64::from_le_bytes(data_portion[1..9].try_into().unwrap());
            let page_id = u64::from_le_bytes(data_portion[9..17].try_into().unwrap());
            let data_len = u32::from_le_bytes(data_portion[17..21].try_into().unwrap()) as usize;

            if let Some(record_type) = rec_type {
                let data = data_portion[21..21 + data_len].to_vec();
                records.push(WalRecord {
                    txn_id,
                    record_type,
                    page_id,
                    data,
                });
                debug!("WAL replay: {:?} txn={}", record_type, txn_id);
            } else {
                warn!("WAL: unknown record type at offset {}", pos - 4);
            }

            pos += body_len;
        }

        Ok(records)
    }

    pub fn truncate(&mut self) -> Result<()> {
        self.file.get_mut().set_len(0)?;
        self.file.get_mut().seek(SeekFrom::Start(0))?;
        Ok(())
    }
}

/// Apply committed WAL records to pages, return page data updates
pub fn recover_from_wal(records: &[WalRecord]) -> Vec<(u64, Vec<u8>)> {
    use std::collections::{HashMap, HashSet};

    let mut committed_txns: HashSet<u64> = HashSet::new();
    let mut writes: HashMap<u64, Vec<u8>> = HashMap::new();
    let mut txn_writes: HashMap<u64, Vec<u64>> = HashMap::new();

    for rec in records {
        match rec.record_type {
            WalRecordType::Begin => {}
            WalRecordType::Write => {
                writes.insert(rec.page_id, rec.data.clone());
                txn_writes.entry(rec.txn_id).or_default().push(rec.page_id);
            }
            WalRecordType::Commit => {
                committed_txns.insert(rec.txn_id);
            }
            WalRecordType::Rollback => {
                // Discard writes from rolled-back txn
                if let Some(pages) = txn_writes.remove(&rec.txn_id) {
                    for pid in pages {
                        writes.remove(&pid);
                    }
                }
            }
            WalRecordType::Checkpoint => {}
        }
    }

    // Only return writes from committed transactions
    let mut result = Vec::new();
    for (txn_id, pages) in &txn_writes {
        if committed_txns.contains(txn_id) {
            for pid in pages {
                if let Some(data) = writes.get(pid) {
                    result.push((*pid, data.clone()));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_wal_write_and_replay() {
        let tmp = NamedTempFile::new().unwrap();
        {
            let mut wal = Wal::open(tmp.path()).unwrap();
            wal.append(&WalRecord {
                txn_id: 1,
                record_type: WalRecordType::Begin,
                page_id: 0,
                data: vec![],
            })
            .unwrap();
            wal.append(&WalRecord {
                txn_id: 1,
                record_type: WalRecordType::Write,
                page_id: 5,
                data: vec![1, 2, 3, 4],
            })
            .unwrap();
            wal.append(&WalRecord {
                txn_id: 1,
                record_type: WalRecordType::Commit,
                page_id: 0,
                data: vec![],
            })
            .unwrap();
            wal.flush().unwrap();
        }

        let records = Wal::replay(tmp.path()).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[1].page_id, 5);
        assert_eq!(records[1].data, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_recover_committed_only() {
        let records = vec![
            WalRecord {
                txn_id: 1,
                record_type: WalRecordType::Begin,
                page_id: 0,
                data: vec![],
            },
            WalRecord {
                txn_id: 1,
                record_type: WalRecordType::Write,
                page_id: 10,
                data: vec![99],
            },
            WalRecord {
                txn_id: 1,
                record_type: WalRecordType::Commit,
                page_id: 0,
                data: vec![],
            },
            WalRecord {
                txn_id: 2,
                record_type: WalRecordType::Begin,
                page_id: 0,
                data: vec![],
            },
            WalRecord {
                txn_id: 2,
                record_type: WalRecordType::Write,
                page_id: 20,
                data: vec![88],
            },
            // txn 2 not committed (crash)
        ];

        let result = recover_from_wal(&records);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, 10);
        assert_eq!(result[0].1, vec![99]);
    }

    #[test]
    fn test_wal_crc_corruption_stops_replay() {
        let tmp = NamedTempFile::new().unwrap();
        {
            let mut wal = Wal::open(tmp.path()).unwrap();
            wal.append(&WalRecord {
                txn_id: 1,
                record_type: WalRecordType::Begin,
                page_id: 0,
                data: vec![],
            })
            .unwrap();
            wal.flush().unwrap();
        }
        // Corrupt the file
        {
            let mut f = OpenOptions::new().write(true).open(tmp.path()).unwrap();
            f.seek(SeekFrom::Start(5)).unwrap();
            f.write_all(&[0xFF, 0xFF]).unwrap();
        }
        let records = Wal::replay(tmp.path()).unwrap();
        assert_eq!(records.len(), 0);
    }
}

use crate::error::{MiniDbError, Result};
use log::debug;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};

const WAL_RECORD_TYPES: u8 = 0; // Will be expanded

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalRecord {
    Begin {
        tx_id: u64,
    },
    Insert {
        tx_id: u64,
        table: String,
        row_id: u64,
        data: Vec<u8>,
    },
    Update {
        tx_id: u64,
        table: String,
        row_id: u64,
        old_data: Vec<u8>,
        new_data: Vec<u8>,
    },
    Delete {
        tx_id: u64,
        table: String,
        row_id: u64,
    },
    Commit {
        tx_id: u64,
    },
    Rollback {
        tx_id: u64,
    },
}

impl WalRecord {
    fn record_type(&self) -> u8 {
        match self {
            WalRecord::Begin { .. } => 0x01,
            WalRecord::Insert { .. } => 0x02,
            WalRecord::Update { .. } => 0x03,
            WalRecord::Delete { .. } => 0x04,
            WalRecord::Commit { .. } => 0x05,
            WalRecord::Rollback { .. } => 0x06,
        }
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(self.record_type());
        match self {
            WalRecord::Begin { tx_id }
            | WalRecord::Commit { tx_id }
            | WalRecord::Rollback { tx_id } => {
                buf.extend_from_slice(&tx_id.to_be_bytes());
            }
            WalRecord::Insert {
                tx_id,
                table,
                row_id,
                data,
            }
            | WalRecord::Update {
                tx_id,
                table,
                row_id,
                old_data: _,
                new_data: data,
            } => {
                buf.extend_from_slice(&tx_id.to_be_bytes());
                let table_bytes = table.as_bytes();
                buf.extend_from_slice(&(table_bytes.len() as u16).to_be_bytes());
                buf.extend_from_slice(table_bytes);
                buf.extend_from_slice(&row_id.to_be_bytes());
                buf.extend_from_slice(&(data.len() as u32).to_be_bytes());
                buf.extend_from_slice(data);
            }
            WalRecord::Delete {
                tx_id,
                table,
                row_id,
            } => {
                buf.extend_from_slice(&tx_id.to_be_bytes());
                let table_bytes = table.as_bytes();
                buf.extend_from_slice(&(table_bytes.len() as u16).to_be_bytes());
                buf.extend_from_slice(table_bytes);
                buf.extend_from_slice(&row_id.to_be_bytes());
            }
        }
        buf
    }

    fn serialize_update_with_both(&self) -> Vec<u8> {
        match self {
            WalRecord::Update {
                tx_id,
                table,
                row_id,
                old_data,
                new_data,
            } => {
                let mut buf = Vec::new();
                buf.push(self.record_type());
                buf.extend_from_slice(&tx_id.to_be_bytes());
                let table_bytes = table.as_bytes();
                buf.extend_from_slice(&(table_bytes.len() as u16).to_be_bytes());
                buf.extend_from_slice(table_bytes);
                buf.extend_from_slice(&row_id.to_be_bytes());
                buf.extend_from_slice(&(old_data.len() as u32).to_be_bytes());
                buf.extend_from_slice(old_data);
                buf.extend_from_slice(&(new_data.len() as u32).to_be_bytes());
                buf.extend_from_slice(new_data);
                buf
            }
            _ => self.serialize(),
        }
    }

    fn deserialize(buf: &[u8]) -> Result<Self> {
        if buf.is_empty() {
            return Err(MiniDbError::Storage("Empty WAL record".into()));
        }
        let rec_type = buf[0];
        let mut pos = 1;
        let tx_id = u64::from_be_bytes(
            buf[pos..pos + 8]
                .to_vec()
                .try_into()
                .map_err(|_| MiniDbError::Storage("Invalid tx_id".into()))?,
        );
        pos += 8;
        match rec_type {
            0x01 => Ok(WalRecord::Begin { tx_id }),
            0x05 => Ok(WalRecord::Commit { tx_id }),
            0x06 => Ok(WalRecord::Rollback { tx_id }),
            0x02 | 0x03 => {
                let table_len = u16::from_be_bytes([buf[pos], buf[pos + 1]]) as usize;
                pos += 2;
                let table = String::from_utf8(buf[pos..pos + table_len].to_vec())
                    .map_err(|e| MiniDbError::Storage(e.to_string()))?;
                pos += table_len;
                let row_id = u64::from_be_bytes(
                    buf[pos..pos + 8]
                        .to_vec()
                        .try_into()
                        .map_err(|_| MiniDbError::Storage("Invalid row_id".into()))?,
                );
                pos += 8;
                let data_len =
                    u32::from_be_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]])
                        as usize;
                pos += 4;
                let data = buf[pos..pos + data_len].to_vec();
                if rec_type == 0x02 {
                    Ok(WalRecord::Insert {
                        tx_id,
                        table,
                        row_id,
                        data,
                    })
                } else {
                    // Update with both old and new data
                    pos += data_len;
                    let new_data_len =
                        u32::from_be_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]])
                            as usize;
                    pos += 4;
                    let new_data = buf[pos..pos + new_data_len].to_vec();
                    Ok(WalRecord::Update {
                        tx_id,
                        table,
                        row_id,
                        old_data: data,
                        new_data,
                    })
                }
            }
            0x04 => {
                let table_len = u16::from_be_bytes([buf[pos], buf[pos + 1]]) as usize;
                pos += 2;
                let table = String::from_utf8(buf[pos..pos + table_len].to_vec())
                    .map_err(|e| MiniDbError::Storage(e.to_string()))?;
                pos += table_len;
                let row_id = u64::from_be_bytes(
                    buf[pos..pos + 8]
                        .to_vec()
                        .try_into()
                        .map_err(|_| MiniDbError::Storage("Invalid row_id".into()))?,
                );
                Ok(WalRecord::Delete {
                    tx_id,
                    table,
                    row_id,
                })
            }
            _ => Err(MiniDbError::Storage(format!(
                "Unknown WAL record type: {}",
                rec_type
            ))),
        }
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

pub struct Wal {
    file: File,
    path: String,
    next_tx_id: u64,
}

impl Wal {
    pub fn open(path: &str) -> Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let mut wal = Self {
            file,
            path: path.to_string(),
            next_tx_id: 1,
        };
        // Replay existing WAL to find max tx_id
        wal.replay_no_apply()?;
        Ok(wal)
    }

    fn replay_no_apply(&mut self) -> Result<Vec<WalRecord>> {
        let mut file = OpenOptions::new().read(true).open(&self.path)?;
        let mut records = Vec::new();
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        let mut pos = 0;
        while pos + 8 <= buf.len() {
            // minimum: type(1) + crc(4) + len(2) + at least 1 byte
            let rec_len = u16::from_be_bytes([buf[pos + 5], buf[pos + 6]]) as usize;
            if pos + 7 + rec_len > buf.len() {
                break;
            }
            let rec_data = &buf[pos + 7..pos + 7 + rec_len];
            let stored_crc =
                u32::from_be_bytes([buf[pos + 1], buf[pos + 2], buf[pos + 3], buf[pos + 4]]);
            let computed_crc = crc32(rec_data);
            if stored_crc != computed_crc {
                debug!("CRC mismatch at pos {}, discarding rest of WAL", pos);
                break;
            }
            if let Ok(record) = WalRecord::deserialize(rec_data) {
                if let WalRecord::Begin { tx_id } = &record {
                    self.next_tx_id = self.next_tx_id.max(tx_id + 1);
                }
                records.push(record);
            }
            pos += 7 + rec_len;
        }
        Ok(records)
    }

    pub fn replay(&mut self) -> Result<Vec<WalRecord>> {
        self.replay_no_apply()
    }

    pub fn begin(&mut self, tx_id: u64) -> Result<()> {
        self.write_record(&WalRecord::Begin { tx_id })
    }

    pub fn write_record(&mut self, record: &WalRecord) -> Result<()> {
        let data = record.serialize();
        let crc = crc32(&data);
        let len = data.len() as u16;
        self.file.write_all(&[record.record_type()])?;
        self.file.write_all(&crc.to_be_bytes())?;
        self.file.write_all(&len.to_be_bytes())?;
        self.file.write_all(&data)?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn commit(&mut self, tx_id: u64) -> Result<()> {
        self.write_record(&WalRecord::Commit { tx_id })
    }

    pub fn rollback(&mut self, tx_id: u64) -> Result<()> {
        self.write_record(&WalRecord::Rollback { tx_id })
    }

    pub fn log_insert(
        &mut self,
        tx_id: u64,
        table: &str,
        row_id: u64,
        data: Vec<u8>,
    ) -> Result<()> {
        self.write_record(&WalRecord::Insert {
            tx_id,
            table: table.to_string(),
            row_id,
            data,
        })
    }

    pub fn log_update(
        &mut self,
        tx_id: u64,
        table: &str,
        row_id: u64,
        old_data: Vec<u8>,
        new_data: Vec<u8>,
    ) -> Result<()> {
        self.write_record(&WalRecord::Update {
            tx_id,
            table: table.to_string(),
            row_id,
            old_data,
            new_data,
        })
    }

    pub fn log_delete(&mut self, tx_id: u64, table: &str, row_id: u64) -> Result<()> {
        self.write_record(&WalRecord::Delete {
            tx_id,
            table: table.to_string(),
            row_id,
        })
    }

    pub fn clear(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn next_tx_id(&mut self) -> u64 {
        let id = self.next_tx_id;
        self.next_tx_id += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempfile;

    #[test]
    fn test_crc32() {
        let data = b"hello world";
        let crc = crc32(data);
        assert_ne!(crc, 0);
        assert_eq!(crc, crc32(data));
    }

    #[test]
    fn test_wal_record_serialize_deserialize() {
        let record = WalRecord::Insert {
            tx_id: 1,
            table: "users".into(),
            row_id: 42,
            data: vec![1, 2, 3, 4],
        };
        let serialized = record.serialize();
        let deserialized = WalRecord::deserialize(&serialized).unwrap();
        assert_eq!(record, deserialized);
    }

    #[test]
    fn test_wal_commit_serialize() {
        let record = WalRecord::Commit { tx_id: 5 };
        let serialized = record.serialize();
        let deserialized = WalRecord::deserialize(&serialized).unwrap();
        assert_eq!(record, deserialized);
    }
}

use crc32fast::Hasher;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::{MiniDbError, Result};
use crate::types::Value;

// WAL record types
pub const WAL_INSERT: u8 = 1;
pub const WAL_DELETE: u8 = 2;
pub const WAL_UPDATE: u8 = 3;
pub const WAL_COMMIT: u8 = 4;
pub const WAL_ROLLBACK: u8 = 5;
pub const WAL_CREATE_TABLE: u8 = 6;
pub const WAL_DROP_TABLE: u8 = 7;
pub const WAL_CREATE_INDEX: u8 = 8;
pub const WAL_DROP_INDEX: u8 = 9;
pub const WAL_CHECKPOINT: u8 = 10;

// Format: [length:4][lsn:8][txn_id:8][record_type:1][data_len:4][data...][crc32:4]
// Total overhead: 4 + 8 + 8 + 1 + 4 + 4 = 29 bytes

#[derive(Debug, Clone)]
pub struct WalRecord {
    pub lsn: u64,
    pub txn_id: u64,
    pub record_type: u8,
    pub data: Vec<u8>,
}

pub struct Wal {
    file: File,
    next_lsn: u64,
    offset: u64,
}

impl Wal {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let mut wal = Self {
            file,
            next_lsn: 1,
            offset: 0,
        };

        // Scan to find the highest LSN
        wal.recover_lsn()?;
        Ok(wal)
    }

    fn recover_lsn(&mut self) -> Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut max_lsn = 0u64;
        loop {
            match Self::read_record_from_reader(&mut self.file) {
                Ok(rec) => {
                    if rec.lsn > max_lsn {
                        max_lsn = rec.lsn;
                    }
                }
                Err(_) => break,
            }
        }
        self.next_lsn = max_lsn + 1;
        self.offset = self.file.seek(SeekFrom::End(0))?;
        Ok(())
    }

    pub fn append(&mut self, txn_id: u64, record_type: u8, data: &[u8]) -> Result<u64> {
        let lsn = self.next_lsn;
        self.next_lsn += 1;

        let rec = WalRecord {
            lsn,
            txn_id,
            record_type,
            data: data.to_vec(),
        };

        let encoded = Self::encode_record(&rec);
        self.file.seek(SeekFrom::Start(self.offset))?;
        self.file.write_all(&encoded)?;
        self.file.flush()?;
        self.offset += encoded.len() as u64;

        Ok(lsn)
    }

    pub fn flush(&mut self) -> Result<()> {
        self.file.flush()?;
        self.file.sync_data()?;
        Ok(())
    }

    pub fn read_all_records(&mut self) -> Result<Vec<WalRecord>> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut records = Vec::new();
        loop {
            match Self::read_record_from_reader(&mut self.file) {
                Ok(rec) => records.push(rec),
                Err(_) => break,
            }
        }
        Ok(records)
    }

    pub fn read_committed_records(&mut self) -> Result<Vec<WalRecord>> {
        let all = self.read_all_records()?;
        // Find committed transactions
        let committed: std::collections::HashSet<u64> = all
            .iter()
            .filter(|r| r.record_type == WAL_COMMIT)
            .map(|r| r.txn_id)
            .collect();

        // Return only records from committed transactions, excluding commit/rollback markers
        Ok(all
            .into_iter()
            .filter(|r| {
                committed.contains(&r.txn_id)
                    && r.record_type != WAL_COMMIT
                    && r.record_type != WAL_ROLLBACK
            })
            .collect())
    }

    fn encode_record(rec: &WalRecord) -> Vec<u8> {
        let data_len = rec.data.len() as u32;
        let record_body_len: u32 = 8 + 8 + 1 + 4 + data_len; // lsn + txn_id + type + data_len + data
        let total_len: u32 = record_body_len + 4; // + crc

        let mut buf = Vec::with_capacity(4 + total_len as usize);
        buf.extend_from_slice(&record_body_len.to_le_bytes());
        buf.extend_from_slice(&rec.lsn.to_le_bytes());
        buf.extend_from_slice(&rec.txn_id.to_le_bytes());
        buf.push(rec.record_type);
        buf.extend_from_slice(&data_len.to_le_bytes());
        buf.extend_from_slice(&rec.data);

        let mut hasher = Hasher::new();
        hasher.update(&buf[4..]);
        let crc = hasher.finalize();
        buf.extend_from_slice(&crc.to_le_bytes());

        buf
    }

    fn read_record_from_reader(reader: &mut impl Read) -> Result<WalRecord> {
        let mut len_buf = [0u8; 4];
        reader
            .read_exact(&mut len_buf)
            .map_err(|_| MiniDbError::Wal {
                message: "end of WAL".into(),
            })?;
        let body_len = u32::from_le_bytes(len_buf) as usize;

        let mut body = vec![0u8; body_len];
        reader.read_exact(&mut body).map_err(|_| MiniDbError::Wal {
            message: "truncated WAL record".into(),
        })?;

        let mut crc_buf = [0u8; 4];
        reader
            .read_exact(&mut crc_buf)
            .map_err(|_| MiniDbError::Wal {
                message: "truncated WAL CRC".into(),
            })?;

        let expected_crc = u32::from_le_bytes(crc_buf);
        let mut hasher = Hasher::new();
        hasher.update(&body);
        let actual_crc = hasher.finalize();

        if expected_crc != actual_crc {
            return Err(MiniDbError::Wal {
                message: "WAL CRC mismatch (torn write detected)".into(),
            });
        }

        let lsn = u64::from_le_bytes(body[0..8].try_into().unwrap());
        let txn_id = u64::from_le_bytes(body[8..16].try_into().unwrap());
        let record_type = body[16];
        let data_len = u32::from_le_bytes(body[17..21].try_into().unwrap()) as usize;
        let data = body[21..21 + data_len].to_vec();

        Ok(WalRecord {
            lsn,
            txn_id,
            record_type,
            data,
        })
    }
}

// Helpers for encoding/decoding WAL data payloads
pub fn encode_row_data(table_id: u64, row_id: u64, values: &[Value]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&table_id.to_le_bytes());
    buf.extend_from_slice(&row_id.to_le_bytes());
    let json = serde_json::to_string(values).unwrap_or_default();
    let json_bytes = json.as_bytes();
    buf.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(json_bytes);
    buf
}

pub fn decode_row_data(data: &[u8]) -> (u64, u64, Vec<Value>) {
    let table_id = u64::from_le_bytes(data[0..8].try_into().unwrap());
    let row_id = u64::from_le_bytes(data[8..16].try_into().unwrap());
    let json_len = u32::from_le_bytes(data[16..20].try_into().unwrap()) as usize;
    let json_str = std::str::from_utf8(&data[20..20 + json_len]).unwrap_or("[]");
    let values: Vec<Value> = serde_json::from_str(json_str).unwrap_or_default();
    (table_id, row_id, values)
}

pub fn encode_table_meta(table_id: u64, name: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&table_id.to_le_bytes());
    let name_bytes = name.as_bytes();
    buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(name_bytes);
    buf
}

pub fn encode_index_meta(index_id: u64, table_id: u64, col_idx: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&index_id.to_le_bytes());
    buf.extend_from_slice(&table_id.to_le_bytes());
    buf.extend_from_slice(&col_idx.to_le_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_wal_append_and_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");
        let mut wal = Wal::open(&path).unwrap();

        let data = b"hello";
        let lsn = wal.append(1, WAL_INSERT, data).unwrap();
        assert_eq!(lsn, 1);

        let lsn2 = wal.append(1, WAL_COMMIT, b"").unwrap();
        assert_eq!(lsn2, 2);

        let records = wal.read_all_records().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].record_type, WAL_INSERT);
        assert_eq!(records[0].data, data);
        assert_eq!(records[1].record_type, WAL_COMMIT);
    }

    #[test]
    fn test_wal_committed_only() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");
        let mut wal = Wal::open(&path).unwrap();

        // Committed transaction
        wal.append(1, WAL_INSERT, b"row1").unwrap();
        wal.append(1, WAL_COMMIT, b"").unwrap();

        // Uncommitted transaction
        wal.append(2, WAL_INSERT, b"row2").unwrap();

        let committed = wal.read_committed_records().unwrap();
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].data, b"row1");
    }

    #[test]
    fn test_wal_persistence() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");

        {
            let mut wal = Wal::open(&path).unwrap();
            wal.append(1, WAL_INSERT, b"data").unwrap();
            wal.append(1, WAL_COMMIT, b"").unwrap();
        }

        // Reopen - should recover LSN
        let mut wal = Wal::open(&path).unwrap();
        assert_eq!(wal.next_lsn, 3);
        let lsn = wal.append(2, WAL_INSERT, b"more").unwrap();
        assert_eq!(lsn, 3);
    }

    #[test]
    fn test_encode_decode_row() {
        let values = vec![Value::Int(42), Value::Text("hello".into()), Value::Null];
        let encoded = encode_row_data(1, 10, &values);
        let (tid, rid, decoded) = decode_row_data(&encoded);
        assert_eq!(tid, 1);
        assert_eq!(rid, 10);
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0], Value::Int(42));
        assert_eq!(decoded[1], Value::Text("hello".into()));
        assert_eq!(decoded[2], Value::Null);
    }
}

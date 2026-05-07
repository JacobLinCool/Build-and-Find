/// Write-ahead log (WAL) implementation.
/// Append-only, record-oriented log with CRC32 per record.
///
/// Record format:
///   [total_len: u32]  -- length of everything after this field
///   [crc32: u32]      -- CRC of all bytes after the crc field
///   [record_type: u8]
///   [txn_id: u64]
///   [data: variable]
use crate::error::{MiniDbError, Result};
use crate::storage::heap::RowId;
use crate::types::{deserialize_row, serialize_row, DataType, Row};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const WAL_MAGIC: u32 = 0x574C4731; // "WLG1"
const HEADER_SIZE: usize = 12; // magic(4) + flags(4) + seq(4)

#[repr(u8)]
#[derive(Debug, Clone, PartialEq)]
pub enum WalRecordType {
    Begin = 1,
    Commit = 2,
    Rollback = 3,
    Insert = 4,
    Delete = 5,
    Update = 6,
    Checkpoint = 7,
    IndexInsert = 8,
    IndexDelete = 9,
}

impl WalRecordType {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(WalRecordType::Begin),
            2 => Some(WalRecordType::Commit),
            3 => Some(WalRecordType::Rollback),
            4 => Some(WalRecordType::Insert),
            5 => Some(WalRecordType::Delete),
            6 => Some(WalRecordType::Update),
            7 => Some(WalRecordType::Checkpoint),
            8 => Some(WalRecordType::IndexInsert),
            9 => Some(WalRecordType::IndexDelete),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WalRecord {
    pub txn_id: u64,
    pub body: WalRecordBody,
}

#[derive(Debug, Clone)]
pub enum WalRecordBody {
    Begin,
    Commit,
    Rollback,
    Insert {
        table: String,
        row_id: RowId,
        row: Row,
    },
    Delete {
        table: String,
        row_id: RowId,
        row: Row, // before-image for undo
    },
    Update {
        table: String,
        row_id: RowId,
        before: Row,
        after: Row,
        new_row_id: RowId,
    },
    Checkpoint,
    IndexInsert {
        index: String,
        key_bytes: Vec<u8>,
        row_id: RowId,
    },
    IndexDelete {
        index: String,
        key_bytes: Vec<u8>,
        row_id: RowId,
    },
}

fn encode_string(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u16).to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
}

fn decode_string(data: &[u8], pos: &mut usize) -> Result<String> {
    if *pos + 2 > data.len() {
        return Err(MiniDbError::StorageCorruption(
            "wal string too short".into(),
        ));
    }
    let len = u16::from_le_bytes([data[*pos], data[*pos + 1]]) as usize;
    *pos += 2;
    if *pos + len > data.len() {
        return Err(MiniDbError::StorageCorruption(
            "wal string data truncated".into(),
        ));
    }
    let s = std::str::from_utf8(&data[*pos..*pos + len])
        .map_err(|_| MiniDbError::StorageCorruption("wal string invalid utf8".into()))?
        .to_string();
    *pos += len;
    Ok(s)
}

fn encode_row_id(buf: &mut Vec<u8>, rid: RowId) {
    buf.extend_from_slice(&rid.encode().to_le_bytes());
}

fn decode_row_id(data: &[u8], pos: &mut usize) -> Result<RowId> {
    if *pos + 8 > data.len() {
        return Err(MiniDbError::StorageCorruption(
            "wal row_id truncated".into(),
        ));
    }
    let v = u64::from_le_bytes(data[*pos..*pos + 8].try_into().unwrap());
    *pos += 8;
    Ok(RowId::decode(v))
}

fn encode_row_with_types(buf: &mut Vec<u8>, row: &Row) {
    let serialized = serialize_row(row);
    buf.extend_from_slice(&(serialized.len() as u32).to_le_bytes());
    buf.extend_from_slice(&serialized);
}

fn decode_row_with_types(data: &[u8], pos: &mut usize, types: &[DataType]) -> Result<Row> {
    if *pos + 4 > data.len() {
        return Err(MiniDbError::StorageCorruption(
            "wal row len truncated".into(),
        ));
    }
    let len = u32::from_le_bytes(data[*pos..*pos + 4].try_into().unwrap()) as usize;
    *pos += 4;
    if *pos + len > data.len() {
        return Err(MiniDbError::StorageCorruption(
            "wal row data truncated".into(),
        ));
    }
    let row = deserialize_row(&data[*pos..*pos + len], types)?;
    *pos += len;
    Ok(row)
}

/// Public decode helpers for WAL replay in exec.
pub fn decode_string_pos(data: &[u8], pos: &mut usize) -> Result<String> {
    decode_string(data, pos)
}

pub fn decode_row_id_pos(data: &[u8], pos: &mut usize) -> Result<super::heap::RowId> {
    decode_row_id(data, pos)
}

pub fn decode_row_pos(
    data: &[u8],
    pos: &mut usize,
    types: &[crate::types::DataType],
) -> Result<crate::types::Row> {
    decode_row_with_types(data, pos, types)
}

/// WAL writer -- append records to the log file.
pub struct WalWriter {
    path: PathBuf,
    file: BufWriter<File>,
    pub seq: u64,
}

impl WalWriter {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)?;
        let mut writer = WalWriter {
            path: path.to_path_buf(),
            file: BufWriter::new(file),
            seq: 0,
        };
        Ok(writer)
    }

    pub fn write_record(&mut self, rec: &WalRecord) -> Result<()> {
        let mut body = Vec::new();
        let rtype = match &rec.body {
            WalRecordBody::Begin => WalRecordType::Begin as u8,
            WalRecordBody::Commit => WalRecordType::Commit as u8,
            WalRecordBody::Rollback => WalRecordType::Rollback as u8,
            WalRecordBody::Insert { .. } => WalRecordType::Insert as u8,
            WalRecordBody::Delete { .. } => WalRecordType::Delete as u8,
            WalRecordBody::Update { .. } => WalRecordType::Update as u8,
            WalRecordBody::Checkpoint => WalRecordType::Checkpoint as u8,
            WalRecordBody::IndexInsert { .. } => WalRecordType::IndexInsert as u8,
            WalRecordBody::IndexDelete { .. } => WalRecordType::IndexDelete as u8,
        };
        body.push(rtype);
        body.extend_from_slice(&rec.txn_id.to_le_bytes());

        match &rec.body {
            WalRecordBody::Begin
            | WalRecordBody::Commit
            | WalRecordBody::Rollback
            | WalRecordBody::Checkpoint => {}
            WalRecordBody::Insert { table, row_id, row } => {
                encode_string(&mut body, table);
                encode_row_id(&mut body, *row_id);
                encode_row_with_types(&mut body, row);
            }
            WalRecordBody::Delete { table, row_id, row } => {
                encode_string(&mut body, table);
                encode_row_id(&mut body, *row_id);
                encode_row_with_types(&mut body, row);
            }
            WalRecordBody::Update {
                table,
                row_id,
                before,
                after,
                new_row_id,
            } => {
                encode_string(&mut body, table);
                encode_row_id(&mut body, *row_id);
                encode_row_id(&mut body, *new_row_id);
                encode_row_with_types(&mut body, before);
                encode_row_with_types(&mut body, after);
            }
            WalRecordBody::IndexInsert {
                index,
                key_bytes,
                row_id,
            }
            | WalRecordBody::IndexDelete {
                index,
                key_bytes,
                row_id,
            } => {
                encode_string(&mut body, index);
                encode_row_id(&mut body, *row_id);
                body.extend_from_slice(&(key_bytes.len() as u16).to_le_bytes());
                body.extend_from_slice(key_bytes);
            }
        }

        // Compute CRC over body
        let crc = crc32fast::hash(&body);

        // Write: [total_len: u32][crc32: u32][body...]
        let total_len = (4 + body.len()) as u32; // crc(4) + body
        self.file.write_all(&total_len.to_le_bytes())?;
        self.file.write_all(&crc.to_le_bytes())?;
        self.file.write_all(&body)?;
        self.seq += 1;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.file.flush()?;
        Ok(())
    }
}

/// WAL record with schema-type hints for deserialization.
#[derive(Debug, Clone)]
pub struct RawWalRecord {
    pub txn_id: u64,
    pub rtype: WalRecordType,
    pub data: Vec<u8>, // everything after rtype + txn_id
}

/// Read all raw records from the WAL without interpreting schema.
pub fn read_raw_records(path: &Path) -> Result<Vec<RawWalRecord>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let mut file = File::open(path)?;
    let mut records = Vec::new();

    loop {
        let mut len_buf = [0u8; 4];
        match file.read_exact(&mut len_buf) {
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
        let total_len = u32::from_le_bytes(len_buf) as usize;
        if total_len < 4 {
            break; // truncated
        }

        let mut rest = vec![0u8; total_len];
        match file.read_exact(&mut rest) {
            Err(_) => break, // torn record
            Ok(_) => {}
        }

        let stored_crc = u32::from_le_bytes(rest[0..4].try_into().unwrap());
        let body = &rest[4..];
        let computed_crc = crc32fast::hash(body);

        if stored_crc != computed_crc {
            log::warn!("WAL CRC mismatch, stopping replay");
            break; // torn/corrupted record
        }

        if body.is_empty() {
            break;
        }

        let rtype = match WalRecordType::from_u8(body[0]) {
            Some(t) => t,
            None => {
                log::warn!("Unknown WAL record type {}, stopping", body[0]);
                break;
            }
        };

        if body.len() < 9 {
            break;
        }
        let txn_id = u64::from_le_bytes(body[1..9].try_into().unwrap());
        let data = body[9..].to_vec();

        records.push(RawWalRecord {
            txn_id,
            rtype,
            data,
        });
    }

    Ok(records)
}

/// Determine which transactions committed.
pub fn committed_txns(records: &[RawWalRecord]) -> std::collections::HashSet<u64> {
    let mut committed = std::collections::HashSet::new();
    for rec in records {
        if rec.rtype == WalRecordType::Commit {
            committed.insert(rec.txn_id);
        }
    }
    committed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::heap::RowId;
    use tempfile::NamedTempFile;

    #[test]
    fn test_wal_write_read() {
        let tmp = NamedTempFile::new().unwrap();
        let mut wal = WalWriter::open(tmp.path()).unwrap();

        wal.write_record(&WalRecord {
            txn_id: 1,
            body: WalRecordBody::Begin,
        })
        .unwrap();
        wal.write_record(&WalRecord {
            txn_id: 1,
            body: WalRecordBody::Insert {
                table: "test".into(),
                row_id: RowId::decode(1),
                row: vec![crate::types::Value::Int(42)],
            },
        })
        .unwrap();
        wal.write_record(&WalRecord {
            txn_id: 1,
            body: WalRecordBody::Commit,
        })
        .unwrap();
        wal.flush().unwrap();

        let records = read_raw_records(tmp.path()).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].rtype, WalRecordType::Begin);
        assert_eq!(records[2].rtype, WalRecordType::Commit);

        let committed = committed_txns(&records);
        assert!(committed.contains(&1));
    }

    #[test]
    fn test_crc_detection() {
        let tmp = NamedTempFile::new().unwrap();
        let mut wal = WalWriter::open(tmp.path()).unwrap();
        wal.write_record(&WalRecord {
            txn_id: 99,
            body: WalRecordBody::Begin,
        })
        .unwrap();
        wal.flush().unwrap();

        // Corrupt the file
        let path = tmp.path().to_path_buf();
        let mut content = std::fs::read(&path).unwrap();
        if content.len() > 5 {
            content[5] ^= 0xFF; // corrupt a byte
        }
        std::fs::write(&path, &content).unwrap();

        let records = read_raw_records(&path).unwrap();
        assert_eq!(records.len(), 0); // corrupted record skipped
    }
}

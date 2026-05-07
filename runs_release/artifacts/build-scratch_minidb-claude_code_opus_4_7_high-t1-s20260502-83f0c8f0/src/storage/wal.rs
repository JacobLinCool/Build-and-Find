//! Write-Ahead Log: append-only, record-oriented, CRC32 per record.
//!
//! The WAL holds the logical operations performed by the in-flight or
//! recently-committed transaction. Each record on disk is framed as:
//!
//! ```text
//!   u32 payload_len
//!   u32 crc32(payload)
//!   [payload_len] bytes payload
//! ```
//!
//! `payload[0]` is a record-type tag; the remainder is record-specific.
//! Recovery scans the file, drops any record whose CRC fails or whose framing
//! is truncated, and stops at the first such failure. The list of records
//! between a `Begin` and a matching `Commit` is the committed set; everything
//! after the last `Commit` is treated as uncommitted and dropped.

use crate::error::{MiniDbError, Result};
use crate::storage::codec;
use crate::types::{ColumnDef, DataType, Value};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Logical record types written to the WAL.
#[derive(Debug, Clone, PartialEq)]
pub enum WalRecord {
    Begin {
        txn_id: u64,
    },
    Commit {
        txn_id: u64,
    },
    /// A heap insert of `values` into `table`.
    Insert {
        txn_id: u64,
        table: String,
        values: Vec<Value>,
    },
    /// A heap delete of the row identified by `(values)` (matched by all-column equality).
    /// Storing the full pre-image makes replay independent of any specific row id.
    Delete {
        txn_id: u64,
        table: String,
        values: Vec<Value>,
    },
    /// A heap update: `old` and `new` are the full pre/post images.
    Update {
        txn_id: u64,
        table: String,
        old: Vec<Value>,
        new: Vec<Value>,
    },
    /// DDL: create a table.
    CreateTable {
        txn_id: u64,
        name: String,
        columns: Vec<ColumnDef>,
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
}

impl WalRecord {
    fn encode(&self) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(64);
        match self {
            WalRecord::Begin { txn_id } => {
                buf.push(0x01);
                buf.extend_from_slice(&txn_id.to_le_bytes());
            }
            WalRecord::Commit { txn_id } => {
                buf.push(0x02);
                buf.extend_from_slice(&txn_id.to_le_bytes());
            }
            WalRecord::Insert {
                txn_id,
                table,
                values,
            } => {
                buf.push(0x10);
                buf.extend_from_slice(&txn_id.to_le_bytes());
                write_str(&mut buf, table);
                codec::encode_row(values, &mut buf)?;
            }
            WalRecord::Delete {
                txn_id,
                table,
                values,
            } => {
                buf.push(0x11);
                buf.extend_from_slice(&txn_id.to_le_bytes());
                write_str(&mut buf, table);
                codec::encode_row(values, &mut buf)?;
            }
            WalRecord::Update {
                txn_id,
                table,
                old,
                new,
            } => {
                buf.push(0x12);
                buf.extend_from_slice(&txn_id.to_le_bytes());
                write_str(&mut buf, table);
                codec::encode_row(old, &mut buf)?;
                codec::encode_row(new, &mut buf)?;
            }
            WalRecord::CreateTable {
                txn_id,
                name,
                columns,
            } => {
                buf.push(0x20);
                buf.extend_from_slice(&txn_id.to_le_bytes());
                write_str(&mut buf, name);
                buf.push(columns.len() as u8);
                for c in columns {
                    write_str(&mut buf, &c.name);
                    buf.push(match c.data_type {
                        DataType::Int => 0,
                        DataType::Real => 1,
                        DataType::Text => 2,
                        DataType::Bool => 3,
                    });
                    let mut flags = 0u8;
                    if c.primary_key {
                        flags |= 0b0001;
                    }
                    if c.not_null {
                        flags |= 0b0010;
                    }
                    if c.unique {
                        flags |= 0b0100;
                    }
                    if c.default.is_some() {
                        flags |= 0b1000;
                    }
                    buf.push(flags);
                    if let Some(d) = &c.default {
                        codec::encode_value(d, &mut buf)?;
                    }
                }
            }
            WalRecord::DropTable { txn_id, name } => {
                buf.push(0x21);
                buf.extend_from_slice(&txn_id.to_le_bytes());
                write_str(&mut buf, name);
            }
            WalRecord::CreateIndex {
                txn_id,
                name,
                table,
                column,
            } => {
                buf.push(0x22);
                buf.extend_from_slice(&txn_id.to_le_bytes());
                write_str(&mut buf, name);
                write_str(&mut buf, table);
                write_str(&mut buf, column);
            }
            WalRecord::DropIndex { txn_id, name } => {
                buf.push(0x23);
                buf.extend_from_slice(&txn_id.to_le_bytes());
                write_str(&mut buf, name);
            }
        }
        Ok(buf)
    }

    fn decode(buf: &[u8]) -> Result<Self> {
        if buf.is_empty() {
            return Err(MiniDbError::wal("empty wal payload"));
        }
        let tag = buf[0];
        let mut pos = 1;
        let txn_id = read_u64(buf, &mut pos)?;
        match tag {
            0x01 => Ok(WalRecord::Begin { txn_id }),
            0x02 => Ok(WalRecord::Commit { txn_id }),
            0x10 => {
                let table = read_str(buf, &mut pos)?;
                let (values, used) = codec::decode_row(&buf[pos..])?;
                pos += used;
                let _ = pos;
                Ok(WalRecord::Insert {
                    txn_id,
                    table,
                    values,
                })
            }
            0x11 => {
                let table = read_str(buf, &mut pos)?;
                let (values, _) = codec::decode_row(&buf[pos..])?;
                Ok(WalRecord::Delete {
                    txn_id,
                    table,
                    values,
                })
            }
            0x12 => {
                let table = read_str(buf, &mut pos)?;
                let (old, used1) = codec::decode_row(&buf[pos..])?;
                pos += used1;
                let (new, _) = codec::decode_row(&buf[pos..])?;
                Ok(WalRecord::Update {
                    txn_id,
                    table,
                    old,
                    new,
                })
            }
            0x20 => {
                let name = read_str(buf, &mut pos)?;
                let n = buf
                    .get(pos)
                    .copied()
                    .ok_or_else(|| MiniDbError::wal("truncated CreateTable"))?;
                pos += 1;
                let mut columns = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    let cname = read_str(buf, &mut pos)?;
                    let dt = buf
                        .get(pos)
                        .copied()
                        .ok_or_else(|| MiniDbError::wal("truncated dt"))?;
                    pos += 1;
                    let dt = match dt {
                        0 => DataType::Int,
                        1 => DataType::Real,
                        2 => DataType::Text,
                        3 => DataType::Bool,
                        _ => return Err(MiniDbError::wal("bad data type tag")),
                    };
                    let flags = buf
                        .get(pos)
                        .copied()
                        .ok_or_else(|| MiniDbError::wal("truncated flags"))?;
                    pos += 1;
                    let mut col = ColumnDef::new(cname, dt);
                    col.primary_key = flags & 0b0001 != 0;
                    col.not_null = flags & 0b0010 != 0;
                    col.unique = flags & 0b0100 != 0;
                    if flags & 0b1000 != 0 {
                        let (v, used) = codec::decode_value(&buf[pos..])?;
                        pos += used;
                        col.default = Some(v);
                    }
                    columns.push(col);
                }
                Ok(WalRecord::CreateTable {
                    txn_id,
                    name,
                    columns,
                })
            }
            0x21 => {
                let name = read_str(buf, &mut pos)?;
                Ok(WalRecord::DropTable { txn_id, name })
            }
            0x22 => {
                let name = read_str(buf, &mut pos)?;
                let table = read_str(buf, &mut pos)?;
                let column = read_str(buf, &mut pos)?;
                Ok(WalRecord::CreateIndex {
                    txn_id,
                    name,
                    table,
                    column,
                })
            }
            0x23 => {
                let name = read_str(buf, &mut pos)?;
                Ok(WalRecord::DropIndex { txn_id, name })
            }
            other => Err(MiniDbError::wal(format!("unknown wal tag: {}", other))),
        }
    }

    /// Returns the transaction id this record belongs to.
    pub fn txn_id(&self) -> u64 {
        match self {
            WalRecord::Begin { txn_id }
            | WalRecord::Commit { txn_id }
            | WalRecord::Insert { txn_id, .. }
            | WalRecord::Delete { txn_id, .. }
            | WalRecord::Update { txn_id, .. }
            | WalRecord::CreateTable { txn_id, .. }
            | WalRecord::DropTable { txn_id, .. }
            | WalRecord::CreateIndex { txn_id, .. }
            | WalRecord::DropIndex { txn_id, .. } => *txn_id,
        }
    }

    pub fn is_commit(&self) -> bool {
        matches!(self, WalRecord::Commit { .. })
    }
}

fn write_str(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
}

fn read_str(buf: &[u8], pos: &mut usize) -> Result<String> {
    if buf.len() < *pos + 4 {
        return Err(MiniDbError::wal("truncated string len"));
    }
    let mut a = [0u8; 4];
    a.copy_from_slice(&buf[*pos..*pos + 4]);
    let n = u32::from_le_bytes(a) as usize;
    *pos += 4;
    if buf.len() < *pos + n {
        return Err(MiniDbError::wal("truncated string body"));
    }
    let s = std::str::from_utf8(&buf[*pos..*pos + n])
        .map_err(|_| MiniDbError::wal("invalid utf8 in wal string"))?
        .to_string();
    *pos += n;
    Ok(s)
}

fn read_u64(buf: &[u8], pos: &mut usize) -> Result<u64> {
    if buf.len() < *pos + 8 {
        return Err(MiniDbError::wal("truncated u64"));
    }
    let mut a = [0u8; 8];
    a.copy_from_slice(&buf[*pos..*pos + 8]);
    *pos += 8;
    Ok(u64::from_le_bytes(a))
}

/// The on-disk WAL writer.
pub struct WalWriter {
    file: File,
    path: PathBuf,
}

impl WalWriter {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path: PathBuf = path.as_ref().into();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        Ok(WalWriter { file, path })
    }

    pub fn append(&mut self, record: &WalRecord) -> Result<()> {
        let payload = record.encode()?;
        let mut crc = crc32fast::Hasher::new();
        crc.update(&payload);
        let crc = crc.finalize();
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&(payload.len() as u32).to_le_bytes())?;
        self.file.write_all(&crc.to_le_bytes())?;
        self.file.write_all(&payload)?;
        Ok(())
    }

    pub fn fsync(&mut self) -> Result<()> {
        self.file.sync_data()?;
        Ok(())
    }

    pub fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.sync_data()?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Read all valid records from the WAL on disk, stopping at the first framing
/// or CRC failure (which is treated as a torn write at the tail).
pub fn read_all<P: AsRef<Path>>(path: P) -> Result<Vec<WalRecord>> {
    let mut out = Vec::new();
    let path = path.as_ref();
    if !path.exists() {
        return Ok(out);
    }
    let mut file = OpenOptions::new().read(true).open(path)?;
    let len = file.metadata()?.len();
    let mut pos = 0u64;
    while pos < len {
        let mut hdr = [0u8; 8];
        if let Err(_) = file.read_exact(&mut hdr) {
            break;
        }
        let mut a = [0u8; 4];
        a.copy_from_slice(&hdr[0..4]);
        let plen = u32::from_le_bytes(a) as u64;
        a.copy_from_slice(&hdr[4..8]);
        let crc_expected = u32::from_le_bytes(a);
        if pos + 8 + plen > len {
            break;
        }
        let mut payload = vec![0u8; plen as usize];
        if let Err(_) = file.read_exact(&mut payload) {
            break;
        }
        let mut h = crc32fast::Hasher::new();
        h.update(&payload);
        if h.finalize() != crc_expected {
            break;
        }
        match WalRecord::decode(&payload) {
            Ok(r) => out.push(r),
            Err(_) => break,
        }
        pos += 8 + plen;
    }
    Ok(out)
}

/// Returns only the records belonging to transactions that fully committed.
/// Records of in-flight / aborted transactions are dropped.
pub fn committed_records(records: Vec<WalRecord>) -> Vec<WalRecord> {
    use std::collections::{HashMap, HashSet};
    let mut by_txn: HashMap<u64, Vec<WalRecord>> = HashMap::new();
    let mut committed: HashSet<u64> = HashSet::new();
    let mut order: Vec<u64> = Vec::new();
    for r in records {
        let id = r.txn_id();
        if r.is_commit() {
            committed.insert(id);
            continue;
        }
        let entry = by_txn.entry(id).or_default();
        if entry.is_empty() {
            order.push(id);
        }
        entry.push(r);
    }
    let mut out = Vec::new();
    for id in order {
        if committed.contains(&id) {
            if let Some(rs) = by_txn.remove(&id) {
                for r in rs {
                    if !matches!(r, WalRecord::Begin { .. }) {
                        out.push(r);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = temp_dir();
        p.push(format!("minidb_wal_{}_{}.log", std::process::id(), n));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn round_trip_records() {
        let path = tmp();
        {
            let mut w = WalWriter::open(&path).unwrap();
            w.append(&WalRecord::Begin { txn_id: 1 }).unwrap();
            w.append(&WalRecord::Insert {
                txn_id: 1,
                table: "t".into(),
                values: vec![Value::Int(7), Value::Text("x".into())],
            })
            .unwrap();
            w.append(&WalRecord::Commit { txn_id: 1 }).unwrap();
            w.fsync().unwrap();
        }
        let records = read_all(&path).unwrap();
        assert_eq!(records.len(), 3);
        let committed = committed_records(records);
        assert_eq!(committed.len(), 1);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn truncated_tail_dropped() {
        let path = tmp();
        {
            let mut w = WalWriter::open(&path).unwrap();
            w.append(&WalRecord::Begin { txn_id: 1 }).unwrap();
            w.append(&WalRecord::Insert {
                txn_id: 1,
                table: "t".into(),
                values: vec![Value::Int(7)],
            })
            .unwrap();
            w.append(&WalRecord::Commit { txn_id: 1 }).unwrap();
            w.fsync().unwrap();
        }
        // Append junk to simulate a torn write.
        {
            let mut f = OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(&[0xff, 0xff, 0xff, 0xff, 0xaa, 0xbb, 0xcc, 0xdd])
                .unwrap();
        }
        let records = read_all(&path).unwrap();
        assert_eq!(records.len(), 3);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn uncommitted_dropped() {
        let path = tmp();
        {
            let mut w = WalWriter::open(&path).unwrap();
            w.append(&WalRecord::Begin { txn_id: 7 }).unwrap();
            w.append(&WalRecord::Insert {
                txn_id: 7,
                table: "t".into(),
                values: vec![Value::Int(1)],
            })
            .unwrap();
            // No commit
            w.fsync().unwrap();
        }
        let records = read_all(&path).unwrap();
        let committed = committed_records(records);
        assert!(committed.is_empty());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn rowid_size_unused_warning() {
        use crate::storage::page::{PageId, RowId};
        let _ = (PageId(1), RowId::new(PageId(1), 0));
    }
}

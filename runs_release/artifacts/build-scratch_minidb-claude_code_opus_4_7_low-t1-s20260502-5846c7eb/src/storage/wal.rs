//! Append-only write-ahead log with per-record CRC32 framing.
//!
//! Each record on disk is:
//!   length (u32 LE) | crc32 (u32 LE, of payload only) | payload (length bytes)
//!
//! Payload is a typed record encoded by [`WalRecord::encode`].

use crate::error::{MiniDbError, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
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
    Insert {
        txn_id: u64,
        table: String,
        page: u64,
        slot: u16,
        tuple: Vec<u8>,
    },
    Delete {
        txn_id: u64,
        table: String,
        page: u64,
        slot: u16,
    },
    Update {
        txn_id: u64,
        table: String,
        page: u64,
        slot: u16,
        new: Vec<u8>,
    },
    CreateTable {
        txn_id: u64,
        schema_blob: Vec<u8>,
    },
    DropTable {
        txn_id: u64,
        table: String,
    },
    CreateIndex {
        txn_id: u64,
        blob: Vec<u8>,
    },
    DropIndex {
        txn_id: u64,
        name: String,
    },
}

impl WalRecord {
    fn tag(&self) -> u8 {
        match self {
            WalRecord::Begin { .. } => 1,
            WalRecord::Commit { .. } => 2,
            WalRecord::Rollback { .. } => 3,
            WalRecord::Insert { .. } => 4,
            WalRecord::Delete { .. } => 5,
            WalRecord::Update { .. } => 6,
            WalRecord::CreateTable { .. } => 7,
            WalRecord::DropTable { .. } => 8,
            WalRecord::CreateIndex { .. } => 9,
            WalRecord::DropIndex { .. } => 10,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        out.push(self.tag());
        match self {
            WalRecord::Begin { txn_id }
            | WalRecord::Commit { txn_id }
            | WalRecord::Rollback { txn_id } => {
                out.extend_from_slice(&txn_id.to_le_bytes());
            }
            WalRecord::Insert {
                txn_id,
                table,
                page,
                slot,
                tuple,
            } => {
                out.extend_from_slice(&txn_id.to_le_bytes());
                write_str(&mut out, table);
                out.extend_from_slice(&page.to_le_bytes());
                out.extend_from_slice(&slot.to_le_bytes());
                write_blob(&mut out, tuple);
            }
            WalRecord::Delete {
                txn_id,
                table,
                page,
                slot,
            } => {
                out.extend_from_slice(&txn_id.to_le_bytes());
                write_str(&mut out, table);
                out.extend_from_slice(&page.to_le_bytes());
                out.extend_from_slice(&slot.to_le_bytes());
            }
            WalRecord::Update {
                txn_id,
                table,
                page,
                slot,
                new,
            } => {
                out.extend_from_slice(&txn_id.to_le_bytes());
                write_str(&mut out, table);
                out.extend_from_slice(&page.to_le_bytes());
                out.extend_from_slice(&slot.to_le_bytes());
                write_blob(&mut out, new);
            }
            WalRecord::CreateTable {
                txn_id,
                schema_blob,
            } => {
                out.extend_from_slice(&txn_id.to_le_bytes());
                write_blob(&mut out, schema_blob);
            }
            WalRecord::DropTable { txn_id, table } => {
                out.extend_from_slice(&txn_id.to_le_bytes());
                write_str(&mut out, table);
            }
            WalRecord::CreateIndex { txn_id, blob } => {
                out.extend_from_slice(&txn_id.to_le_bytes());
                write_blob(&mut out, blob);
            }
            WalRecord::DropIndex { txn_id, name } => {
                out.extend_from_slice(&txn_id.to_le_bytes());
                write_str(&mut out, name);
            }
        }
        out
    }

    pub fn decode(buf: &[u8]) -> Result<WalRecord> {
        if buf.is_empty() {
            return Err(MiniDbError::Wal("empty record".into()));
        }
        let tag = buf[0];
        let mut p = 1;
        let txn_id = read_u64(buf, &mut p)?;
        match tag {
            1 => Ok(WalRecord::Begin { txn_id }),
            2 => Ok(WalRecord::Commit { txn_id }),
            3 => Ok(WalRecord::Rollback { txn_id }),
            4 => {
                let table = read_str(buf, &mut p)?;
                let page = read_u64(buf, &mut p)?;
                let slot = read_u16(buf, &mut p)?;
                let tuple = read_blob(buf, &mut p)?;
                Ok(WalRecord::Insert {
                    txn_id,
                    table,
                    page,
                    slot,
                    tuple,
                })
            }
            5 => {
                let table = read_str(buf, &mut p)?;
                let page = read_u64(buf, &mut p)?;
                let slot = read_u16(buf, &mut p)?;
                Ok(WalRecord::Delete {
                    txn_id,
                    table,
                    page,
                    slot,
                })
            }
            6 => {
                let table = read_str(buf, &mut p)?;
                let page = read_u64(buf, &mut p)?;
                let slot = read_u16(buf, &mut p)?;
                let new = read_blob(buf, &mut p)?;
                Ok(WalRecord::Update {
                    txn_id,
                    table,
                    page,
                    slot,
                    new,
                })
            }
            7 => {
                let blob = read_blob(buf, &mut p)?;
                Ok(WalRecord::CreateTable {
                    txn_id,
                    schema_blob: blob,
                })
            }
            8 => {
                let table = read_str(buf, &mut p)?;
                Ok(WalRecord::DropTable { txn_id, table })
            }
            9 => {
                let blob = read_blob(buf, &mut p)?;
                Ok(WalRecord::CreateIndex { txn_id, blob })
            }
            10 => {
                let name = read_str(buf, &mut p)?;
                Ok(WalRecord::DropIndex { txn_id, name })
            }
            other => Err(MiniDbError::Wal(format!("unknown tag {}", other))),
        }
    }

    pub fn txn_id(&self) -> u64 {
        match self {
            WalRecord::Begin { txn_id }
            | WalRecord::Commit { txn_id }
            | WalRecord::Rollback { txn_id }
            | WalRecord::Insert { txn_id, .. }
            | WalRecord::Delete { txn_id, .. }
            | WalRecord::Update { txn_id, .. }
            | WalRecord::CreateTable { txn_id, .. }
            | WalRecord::DropTable { txn_id, .. }
            | WalRecord::CreateIndex { txn_id, .. }
            | WalRecord::DropIndex { txn_id, .. } => *txn_id,
        }
    }
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    write_blob(out, s.as_bytes());
}
fn write_blob(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}
fn read_u16(buf: &[u8], p: &mut usize) -> Result<u16> {
    if *p + 2 > buf.len() {
        return Err(MiniDbError::Wal("truncated u16".into()));
    }
    let mut a = [0u8; 2];
    a.copy_from_slice(&buf[*p..*p + 2]);
    *p += 2;
    Ok(u16::from_le_bytes(a))
}
fn read_u64(buf: &[u8], p: &mut usize) -> Result<u64> {
    if *p + 8 > buf.len() {
        return Err(MiniDbError::Wal("truncated u64".into()));
    }
    let mut a = [0u8; 8];
    a.copy_from_slice(&buf[*p..*p + 8]);
    *p += 8;
    Ok(u64::from_le_bytes(a))
}
fn read_blob(buf: &[u8], p: &mut usize) -> Result<Vec<u8>> {
    if *p + 4 > buf.len() {
        return Err(MiniDbError::Wal("truncated blob len".into()));
    }
    let mut a = [0u8; 4];
    a.copy_from_slice(&buf[*p..*p + 4]);
    let n = u32::from_le_bytes(a) as usize;
    *p += 4;
    if *p + n > buf.len() {
        return Err(MiniDbError::Wal("truncated blob payload".into()));
    }
    let v = buf[*p..*p + n].to_vec();
    *p += n;
    Ok(v)
}
fn read_str(buf: &[u8], p: &mut usize) -> Result<String> {
    let b = read_blob(buf, p)?;
    String::from_utf8(b).map_err(|_| MiniDbError::Wal("invalid utf8 in wal".into()))
}

pub struct Wal {
    file: File,
    path: PathBuf,
}

impl Wal {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let p = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .append(false)
            .open(&p)?;
        Ok(Wal { file, path: p })
    }

    pub fn append(&mut self, rec: &WalRecord) -> Result<()> {
        let payload = rec.encode();
        let crc = crc32fast::hash(&payload);
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&(payload.len() as u32).to_le_bytes())?;
        self.file.write_all(&crc.to_le_bytes())?;
        self.file.write_all(&payload)?;
        Ok(())
    }

    pub fn fsync(&mut self) -> Result<()> {
        self.file.sync_all()?;
        Ok(())
    }

    pub fn read_all(&mut self) -> Result<Vec<WalRecord>> {
        let mut out = Vec::new();
        self.file.seek(SeekFrom::Start(0))?;
        let mut hdr = [0u8; 8];
        loop {
            match self.file.read_exact(&mut hdr) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
            let len = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
            let crc = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
            let mut payload = vec![0u8; len];
            match self.file.read_exact(&mut payload) {
                Ok(()) => {}
                Err(_) => break, // torn write at end
            }
            let actual_crc = crc32fast::hash(&payload);
            if actual_crc != crc {
                log::warn!("WAL crc mismatch — stopping replay at this record");
                break;
            }
            match WalRecord::decode(&payload) {
                Ok(r) => out.push(r),
                Err(e) => {
                    log::warn!("WAL decode error: {} — stopping replay", e);
                    break;
                }
            }
        }
        Ok(out)
    }

    pub fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.sync_all()?;
        self.file.seek(SeekFrom::Start(0))?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Replay records, return only those belonging to fully-committed transactions
/// (i.e. ones with a Commit record present).
pub fn committed_records(records: Vec<WalRecord>) -> Vec<WalRecord> {
    use std::collections::HashSet;
    let mut committed: HashSet<u64> = HashSet::new();
    for r in &records {
        if let WalRecord::Commit { txn_id } = r {
            committed.insert(*txn_id);
        }
    }
    records
        .into_iter()
        .filter(|r| committed.contains(&r.txn_id()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn append_and_replay() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("w.log");
        {
            let mut wal = Wal::open(&p).unwrap();
            wal.append(&WalRecord::Begin { txn_id: 1 }).unwrap();
            wal.append(&WalRecord::Insert {
                txn_id: 1,
                table: "t".into(),
                page: 1,
                slot: 0,
                tuple: vec![1, 2, 3],
            })
            .unwrap();
            wal.append(&WalRecord::Commit { txn_id: 1 }).unwrap();
            wal.fsync().unwrap();
        }
        let mut wal = Wal::open(&p).unwrap();
        let recs = wal.read_all().unwrap();
        assert_eq!(recs.len(), 3);
        let committed = committed_records(recs);
        assert_eq!(committed.len(), 3);
    }

    #[test]
    fn uncommitted_filtered_out() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("w.log");
        let mut wal = Wal::open(&p).unwrap();
        wal.append(&WalRecord::Begin { txn_id: 1 }).unwrap();
        wal.append(&WalRecord::Insert {
            txn_id: 1,
            table: "t".into(),
            page: 1,
            slot: 0,
            tuple: vec![9],
        })
        .unwrap();
        // No commit
        wal.fsync().unwrap();
        let recs = wal.read_all().unwrap();
        let committed = committed_records(recs);
        assert!(committed.is_empty());
    }
}

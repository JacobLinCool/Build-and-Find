//! Write-Ahead Log: append-only, record-oriented, with a CRC32 per record.
//!
//! Record framing (little-endian):
//!   u32  record length (header excluded; covers `crc..end`)
//!   u32  crc32 of the payload
//!   u8   record type
//!   u64  transaction id
//!   ...  payload (record-type specific)
//!
//! Record types:
//!   1 = Begin
//!   2 = Commit
//!   3 = Rollback
//!   4 = PageImage      (u64 page_id, [PAGE_SIZE bytes])
//!   5 = CatalogSnapshot (u32 len, [bytes])
//!
//! Replay rule: only fully-committed transactions are redone. Their page
//! images are written to the data file; their catalog snapshot is written
//! atomically. Pending (uncommitted) transactions and torn tail records
//! are discarded.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use log::warn;

use crate::error::{MiniDbError, Result};

use super::page::{PageId, PAGE_SIZE};

pub const REC_BEGIN: u8 = 1;
pub const REC_COMMIT: u8 = 2;
pub const REC_ROLLBACK: u8 = 3;
pub const REC_PAGE_IMAGE: u8 = 4;
pub const REC_CATALOG: u8 = 5;

#[derive(Clone, Debug, PartialEq)]
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
    PageImage {
        txn_id: u64,
        page_id: PageId,
        bytes: Vec<u8>,
    },
    CatalogSnapshot {
        txn_id: u64,
        bytes: Vec<u8>,
    },
}

impl WalRecord {
    fn txn_id(&self) -> u64 {
        match self {
            Self::Begin { txn_id }
            | Self::Commit { txn_id }
            | Self::Rollback { txn_id }
            | Self::PageImage { txn_id, .. }
            | Self::CatalogSnapshot { txn_id, .. } => *txn_id,
        }
    }
}

pub struct Wal {
    path: PathBuf,
    file: File,
}

impl Wal {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;
        Ok(Self { path, file })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&mut self, rec: &WalRecord) -> Result<()> {
        let payload = encode_payload(rec);
        let crc = crc32fast::hash(&payload);
        let mut header = Vec::with_capacity(8);
        header.extend_from_slice(&(payload.len() as u32 + 4).to_le_bytes());
        header.extend_from_slice(&crc.to_le_bytes());
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&header)?;
        self.file.write_all(&payload)?;
        Ok(())
    }

    pub fn fsync(&mut self) -> Result<()> {
        self.file.sync_all()?;
        Ok(())
    }

    /// Read every record from the start of the file. Stops cleanly on
    /// torn-tail records (truncated header or CRC mismatch).
    pub fn read_all(&mut self) -> Result<Vec<WalRecord>> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut out = Vec::new();
        loop {
            let mut hdr = [0u8; 8];
            match self.file.read_exact(&mut hdr) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
            let rec_len = u32::from_le_bytes(hdr[0..4].try_into().unwrap()) as usize;
            let crc = u32::from_le_bytes(hdr[4..8].try_into().unwrap());
            // rec_len includes the crc (4 bytes already in header), payload follows.
            let payload_len = rec_len.saturating_sub(4);
            let mut payload = vec![0u8; payload_len];
            match self.file.read_exact(&mut payload) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    warn!("truncated WAL tail: dropping last record");
                    break;
                }
                Err(e) => return Err(e.into()),
            }
            let actual = crc32fast::hash(&payload);
            if actual != crc {
                warn!("WAL CRC mismatch: dropping last record");
                break;
            }
            match decode_payload(&payload) {
                Ok(r) => out.push(r),
                Err(e) => {
                    warn!("WAL decode failure: {}", e);
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Truncate the WAL to zero length.
    pub fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.sync_all()?;
        Ok(())
    }
}

fn encode_payload(rec: &WalRecord) -> Vec<u8> {
    let mut buf = Vec::new();
    match rec {
        WalRecord::Begin { txn_id } => {
            buf.push(REC_BEGIN);
            buf.extend_from_slice(&txn_id.to_le_bytes());
        }
        WalRecord::Commit { txn_id } => {
            buf.push(REC_COMMIT);
            buf.extend_from_slice(&txn_id.to_le_bytes());
        }
        WalRecord::Rollback { txn_id } => {
            buf.push(REC_ROLLBACK);
            buf.extend_from_slice(&txn_id.to_le_bytes());
        }
        WalRecord::PageImage {
            txn_id,
            page_id,
            bytes,
        } => {
            buf.push(REC_PAGE_IMAGE);
            buf.extend_from_slice(&txn_id.to_le_bytes());
            buf.extend_from_slice(&page_id.0.to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        WalRecord::CatalogSnapshot { txn_id, bytes } => {
            buf.push(REC_CATALOG);
            buf.extend_from_slice(&txn_id.to_le_bytes());
            buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(bytes);
        }
    }
    buf
}

fn decode_payload(payload: &[u8]) -> Result<WalRecord> {
    if payload.is_empty() {
        return Err(MiniDbError::storage("empty WAL payload"));
    }
    let kind = payload[0];
    if payload.len() < 9 {
        return Err(MiniDbError::storage("short WAL payload"));
    }
    let txn_id = u64::from_le_bytes(payload[1..9].try_into().unwrap());
    match kind {
        REC_BEGIN => Ok(WalRecord::Begin { txn_id }),
        REC_COMMIT => Ok(WalRecord::Commit { txn_id }),
        REC_ROLLBACK => Ok(WalRecord::Rollback { txn_id }),
        REC_PAGE_IMAGE => {
            if payload.len() < 9 + 8 + PAGE_SIZE {
                return Err(MiniDbError::storage("short PageImage record"));
            }
            let page_id = PageId(u64::from_le_bytes(payload[9..17].try_into().unwrap()));
            let bytes = payload[17..17 + PAGE_SIZE].to_vec();
            Ok(WalRecord::PageImage {
                txn_id,
                page_id,
                bytes,
            })
        }
        REC_CATALOG => {
            if payload.len() < 9 + 4 {
                return Err(MiniDbError::storage("short Catalog record"));
            }
            let n = u32::from_le_bytes(payload[9..13].try_into().unwrap()) as usize;
            if payload.len() < 13 + n {
                return Err(MiniDbError::storage("short Catalog body"));
            }
            let bytes = payload[13..13 + n].to_vec();
            Ok(WalRecord::CatalogSnapshot { txn_id, bytes })
        }
        other => Err(MiniDbError::storage(format!(
            "unknown WAL record type {}",
            other
        ))),
    }
}

/// Apply WAL records: replays committed transactions onto `data_file`. Returns
/// the catalog bytes from the most-recent committed catalog snapshot, if any.
pub fn replay(records: Vec<WalRecord>, data_file: &mut File) -> Result<Option<Vec<u8>>> {
    // First: bucket by txn_id and find committed/rolled-back transactions.
    let mut by_txn: HashMap<u64, Vec<WalRecord>> = HashMap::new();
    let mut committed: Vec<u64> = Vec::new();
    let mut rolled: std::collections::HashSet<u64> = std::collections::HashSet::new();
    for r in &records {
        by_txn.entry(r.txn_id()).or_default().push(r.clone());
    }
    // Iterate in original order to preserve commit ordering.
    let mut seen = std::collections::HashSet::new();
    for r in &records {
        match r {
            WalRecord::Commit { txn_id } => {
                if seen.insert(*txn_id) {
                    committed.push(*txn_id);
                }
            }
            WalRecord::Rollback { txn_id } => {
                rolled.insert(*txn_id);
            }
            _ => {}
        }
    }

    let mut latest_catalog: Option<Vec<u8>> = None;
    for txn in committed {
        if rolled.contains(&txn) {
            continue;
        }
        let recs = by_txn.remove(&txn).unwrap_or_default();
        for r in recs {
            match r {
                WalRecord::PageImage { page_id, bytes, .. } => {
                    let needed = (page_id.0) * PAGE_SIZE as u64;
                    let cur_len = data_file.metadata()?.len();
                    if cur_len < needed {
                        data_file.set_len(needed)?;
                    }
                    data_file.seek(SeekFrom::Start((page_id.0 - 1) * PAGE_SIZE as u64))?;
                    data_file.write_all(&bytes)?;
                }
                WalRecord::CatalogSnapshot { bytes, .. } => {
                    latest_catalog = Some(bytes);
                }
                _ => {}
            }
        }
    }
    data_file.sync_all()?;
    Ok(latest_catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn append_and_read_roundtrip() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("wal");
        let mut wal = Wal::open(&p).unwrap();
        wal.append(&WalRecord::Begin { txn_id: 1 }).unwrap();
        wal.append(&WalRecord::PageImage {
            txn_id: 1,
            page_id: PageId(1),
            bytes: vec![7u8; PAGE_SIZE],
        })
        .unwrap();
        wal.append(&WalRecord::Commit { txn_id: 1 }).unwrap();
        wal.fsync().unwrap();
        drop(wal);

        let mut wal = Wal::open(&p).unwrap();
        let recs = wal.read_all().unwrap();
        assert_eq!(recs.len(), 3);
        assert!(matches!(recs[0], WalRecord::Begin { .. }));
        assert!(matches!(recs[1], WalRecord::PageImage { .. }));
        assert!(matches!(recs[2], WalRecord::Commit { .. }));
    }

    #[test]
    fn truncated_tail_is_skipped() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("wal");
        let mut wal = Wal::open(&p).unwrap();
        wal.append(&WalRecord::Begin { txn_id: 1 }).unwrap();
        wal.append(&WalRecord::Commit { txn_id: 1 }).unwrap();
        wal.fsync().unwrap();
        drop(wal);

        // Truncate the file by removing the last 3 bytes.
        let len = std::fs::metadata(&p).unwrap().len();
        let f = OpenOptions::new().write(true).open(&p).unwrap();
        f.set_len(len - 3).unwrap();

        let mut wal = Wal::open(&p).unwrap();
        let recs = wal.read_all().unwrap();
        assert_eq!(recs.len(), 1);
    }

    #[test]
    fn replay_writes_pages() {
        use std::fs::OpenOptions;
        let d = TempDir::new().unwrap();
        let wal_path = d.path().join("wal");
        let data_path = d.path().join("data");
        let mut wal = Wal::open(&wal_path).unwrap();
        wal.append(&WalRecord::Begin { txn_id: 7 }).unwrap();
        let mut img = vec![0u8; PAGE_SIZE];
        img[0] = 0xAB;
        wal.append(&WalRecord::PageImage {
            txn_id: 7,
            page_id: PageId(1),
            bytes: img,
        })
        .unwrap();
        wal.append(&WalRecord::Commit { txn_id: 7 }).unwrap();
        wal.fsync().unwrap();
        drop(wal);

        let mut data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&data_path)
            .unwrap();
        let mut wal = Wal::open(&wal_path).unwrap();
        let recs = wal.read_all().unwrap();
        replay(recs, &mut data).unwrap();
        let mut buf = vec![0u8; PAGE_SIZE];
        data.seek(SeekFrom::Start(0)).unwrap();
        data.read_exact(&mut buf).unwrap();
        assert_eq!(buf[0], 0xAB);
    }
}

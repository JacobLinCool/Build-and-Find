use crc32fast::Hasher;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

use crate::{
    error::{MiniDbError, Result},
    parser::ast::Statement,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WalRecord {
    Begin(u64),
    Statement { tx: u64, statement: Statement },
    Commit(u64),
    Rollback(u64),
}

pub struct Wal {
    path: PathBuf,
}

impl Wal {
    pub fn open(path: PathBuf) -> Result<Self> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        Ok(Self { path })
    }
    pub fn append(&self, record: &WalRecord) -> Result<()> {
        let data = serde_json::to_vec(record).map_err(|e| MiniDbError::Storage(e.to_string()))?;
        let mut hasher = Hasher::new();
        hasher.update(&data);
        let crc = hasher.finalize();
        let mut f = OpenOptions::new().append(true).open(&self.path)?;
        f.write_all(&(data.len() as u32).to_le_bytes())?;
        f.write_all(&crc.to_le_bytes())?;
        f.write_all(&data)?;
        f.sync_data()?;
        Ok(())
    }
    pub fn read_records(&self) -> Result<Vec<WalRecord>> {
        let mut f = File::open(&self.path)?;
        let mut out = Vec::new();
        loop {
            let mut hdr = [0u8; 8];
            match f.read_exact(&mut hdr) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
            let len = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
            let crc = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
            let mut data = vec![0u8; len];
            if f.read_exact(&mut data).is_err() {
                break;
            }
            let mut hasher = Hasher::new();
            hasher.update(&data);
            if hasher.finalize() != crc {
                break;
            }
            out.push(
                serde_json::from_slice(&data).map_err(|e| MiniDbError::Storage(e.to_string()))?,
            );
        }
        Ok(committed_prefix(out))
    }
    pub fn truncate(&self) -> Result<()> {
        let mut f = OpenOptions::new().write(true).open(&self.path)?;
        f.set_len(0)?;
        f.seek(SeekFrom::Start(0))?;
        Ok(())
    }
}

fn committed_prefix(records: Vec<WalRecord>) -> Vec<WalRecord> {
    let mut out = Vec::new();
    let mut buf = Vec::new();
    let mut active = None;
    for rec in records {
        match rec {
            WalRecord::Begin(tx) => {
                active = Some(tx);
                buf.clear();
                buf.push(WalRecord::Begin(tx));
            }
            WalRecord::Statement { tx, statement } if active == Some(tx) => {
                buf.push(WalRecord::Statement { tx, statement })
            }
            WalRecord::Commit(tx) if active == Some(tx) => {
                out.extend(buf.drain(..));
                out.push(WalRecord::Commit(tx));
                active = None;
            }
            WalRecord::Rollback(tx) if active == Some(tx) => {
                buf.clear();
                active = None;
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drops_uncommitted_records() {
        let r = committed_prefix(vec![WalRecord::Begin(1)]);
        assert!(r.is_empty());
    }
}

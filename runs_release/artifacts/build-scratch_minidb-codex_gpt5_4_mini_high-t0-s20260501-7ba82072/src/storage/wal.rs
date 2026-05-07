use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crc32fast::Hasher;
use serde::{Deserialize, Serialize};

use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalKind {
    Begin,
    Statement { sql: String },
    Commit,
    Rollback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalRecord {
    pub txid: u64,
    pub kind: WalKind,
}

#[derive(Debug, Clone)]
pub struct Wal {
    path: PathBuf,
}

impl Wal {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn open_append(&self) -> Result<File> {
        Ok(OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&self.path)?)
    }

    fn open_read(&self) -> Result<File> {
        Ok(OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&self.path)?)
    }

    pub fn append(&self, record: &WalRecord) -> Result<()> {
        let mut file = self.open_append()?;
        let payload =
            bincode::serialize(record).map_err(|err| MiniDbError::Storage(err.to_string()))?;
        let mut hasher = Hasher::new();
        hasher.update(&payload);
        let crc = hasher.finalize();
        file.write_all(&(payload.len() as u32).to_le_bytes())?;
        file.write_all(&payload)?;
        file.write_all(&crc.to_le_bytes())?;
        file.flush()?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<WalRecord>> {
        let mut file = self.open_read()?;
        file.seek(SeekFrom::Start(0))?;
        let mut records = Vec::new();
        loop {
            let mut len_bytes = [0u8; 4];
            match file.read_exact(&mut len_bytes) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(err) => return Err(err.into()),
            }
            let len = u32::from_le_bytes(len_bytes) as usize;
            let mut payload = vec![0u8; len];
            if let Err(err) = file.read_exact(&mut payload) {
                if err.kind() == std::io::ErrorKind::UnexpectedEof {
                    break;
                }
                return Err(err.into());
            }
            let mut crc_bytes = [0u8; 4];
            if let Err(err) = file.read_exact(&mut crc_bytes) {
                if err.kind() == std::io::ErrorKind::UnexpectedEof {
                    break;
                }
                return Err(err.into());
            }
            let crc = u32::from_le_bytes(crc_bytes);
            let mut hasher = Hasher::new();
            hasher.update(&payload);
            if hasher.finalize() != crc {
                break;
            }
            let record: WalRecord = match bincode::deserialize(&payload) {
                Ok(record) => record,
                Err(_) => break,
            };
            records.push(record);
        }
        Ok(records)
    }

    pub fn truncate(&self) -> Result<()> {
        let file = self.open_append()?;
        file.set_len(0)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn wal_roundtrip() {
        let dir = tempdir().unwrap();
        let wal = Wal::new(dir.path().join("db.wal"));
        wal.append(&WalRecord {
            txid: 1,
            kind: WalKind::Begin,
        })
        .unwrap();
        wal.append(&WalRecord {
            txid: 1,
            kind: WalKind::Statement {
                sql: "SELECT 1".into(),
            },
        })
        .unwrap();
        wal.append(&WalRecord {
            txid: 1,
            kind: WalKind::Commit,
        })
        .unwrap();
        assert_eq!(wal.read_all().unwrap().len(), 3);
    }
}

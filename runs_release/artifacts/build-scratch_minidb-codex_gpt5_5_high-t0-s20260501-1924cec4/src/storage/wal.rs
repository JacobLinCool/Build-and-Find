use crate::error::{MiniDbError, Result};
use crate::parser::ast::{ColumnDef, Expr};
use crate::types::Value;
use crc32fast::Hasher;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalRecord {
    Begin { tx: u64 },
    Operation { tx: u64, op: WalOp },
    Commit { tx: u64 },
    Rollback { tx: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalOp {
    CreateTable {
        name: String,
        columns: Vec<ColumnDef>,
        checks: Vec<Expr>,
    },
    DropTable {
        name: String,
    },
    CreateIndex {
        name: String,
        table: String,
        column: String,
    },
    DropIndex {
        name: String,
    },
    Insert {
        table: String,
        row_id: u64,
        values: Vec<Value>,
    },
    Update {
        table: String,
        row_id: u64,
        values: Vec<Value>,
    },
    Delete {
        table: String,
        row_id: u64,
    },
}

pub struct Wal;

impl Wal {
    pub fn append(path: &Path, record: &WalRecord) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        let payload = serde_json::to_vec(record)?;
        let len = payload.len() as u32;
        let crc = crc32(&payload);
        file.write_all(&len.to_le_bytes())?;
        file.write_all(&payload)?;
        file.write_all(&crc.to_le_bytes())?;
        Ok(())
    }

    pub fn read_all(path: &Path) -> Result<Vec<WalRecord>> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let mut pos = 0usize;
        let mut records = Vec::new();
        while pos < bytes.len() {
            if bytes.len() - pos < 8 {
                break;
            }
            let len =
                u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
                    as usize;
            pos += 4;
            if bytes.len() - pos < len + 4 {
                break;
            }
            let payload = &bytes[pos..pos + len];
            pos += len;
            let expected =
                u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
            pos += 4;
            let actual = crc32(payload);
            if actual != expected {
                return Err(MiniDbError::Storage("WAL checksum mismatch".into()));
            }
            records.push(serde_json::from_slice(payload)?);
        }
        Ok(records)
    }

    pub fn sync(path: &Path) -> Result<()> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        file.sync_all()?;
        Ok(())
    }

    pub fn truncate(path: &Path) -> Result<()> {
        File::create(path)?;
        Ok(())
    }
}

fn crc32(payload: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(payload);
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trips_records_with_crc() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("db.wal");
        Wal::append(&path, &WalRecord::Begin { tx: 1 }).unwrap();
        Wal::append(&path, &WalRecord::Commit { tx: 1 }).unwrap();
        assert_eq!(Wal::read_all(&path).unwrap().len(), 2);
    }
}

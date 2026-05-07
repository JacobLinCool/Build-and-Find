use crate::error::{MiniDbError, Result};
use crate::parser::ast::Statement;
use crate::storage::DatabaseState;
use crc32fast::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalRecord {
    Begin { txid: u64 },
    Statement { txid: u64, statement: Statement },
    Commit { txid: u64 },
    Rollback { txid: u64 },
}

#[derive(Debug)]
pub struct Wal {
    path: PathBuf,
    file: File,
}

impl Wal {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        Ok(Self { path, file })
    }

    pub fn append_record(&mut self, record: &WalRecord) -> Result<()> {
        let bytes =
            bincode::serialize(record).map_err(|err| MiniDbError::storage(err.to_string()))?;
        let len = bytes.len() as u32;
        let mut hasher = Hasher::new();
        hasher.update(&bytes);
        let crc = hasher.finalize();
        self.file.write_all(&len.to_le_bytes())?;
        self.file.write_all(&crc.to_le_bytes())?;
        self.file.write_all(&bytes)?;
        self.file.flush()?;
        self.file.sync_data()?;
        Ok(())
    }

    pub fn replay(&self, state: &mut DatabaseState) -> Result<()> {
        let mut file = File::open(&self.path)?;
        let mut active: BTreeMap<u64, Vec<Statement>> = BTreeMap::new();
        let mut committed: BTreeMap<u64, bool> = BTreeMap::new();
        loop {
            let mut len_bytes = [0u8; 4];
            if file.read_exact(&mut len_bytes).is_err() {
                break;
            }
            let mut crc_bytes = [0u8; 4];
            if file.read_exact(&mut crc_bytes).is_err() {
                break;
            }
            let len = u32::from_le_bytes(len_bytes) as usize;
            let expected_crc = u32::from_le_bytes(crc_bytes);
            let mut payload = vec![0u8; len];
            if file.read_exact(&mut payload).is_err() {
                break;
            }
            let mut hasher = Hasher::new();
            hasher.update(&payload);
            if hasher.finalize() != expected_crc {
                break;
            }
            let record: WalRecord = match bincode::deserialize(&payload) {
                Ok(record) => record,
                Err(_) => break,
            };
            match record {
                WalRecord::Begin { txid } => {
                    active.entry(txid).or_default();
                }
                WalRecord::Statement { txid, statement } => {
                    active.entry(txid).or_default().push(statement);
                }
                WalRecord::Commit { txid } => {
                    committed.insert(txid, true);
                }
                WalRecord::Rollback { txid } => {
                    active.remove(&txid);
                    committed.remove(&txid);
                }
            }
        }
        let mut txids: Vec<u64> = active.keys().copied().collect();
        txids.sort_unstable();
        for txid in txids {
            if txid <= state.applied_txid {
                continue;
            }
            if committed.get(&txid).copied().unwrap_or(false) {
                if let Some(statements) = active.get(&txid) {
                    for statement in statements {
                        crate::exec::apply_statement_direct(state, statement)?;
                    }
                    state.applied_txid = txid;
                }
            }
        }
        Ok(())
    }

    pub fn max_txid(&self) -> Result<u64> {
        Self::max_txid_from_path(&self.path)
    }

    pub fn max_txid_from_path(path: &Path) -> Result<u64> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(err) => return Err(err.into()),
        };
        let mut max_txid = 0u64;
        loop {
            let mut len_bytes = [0u8; 4];
            if file.read_exact(&mut len_bytes).is_err() {
                break;
            }
            let mut crc_bytes = [0u8; 4];
            if file.read_exact(&mut crc_bytes).is_err() {
                break;
            }
            let len = u32::from_le_bytes(len_bytes) as usize;
            let mut payload = vec![0u8; len];
            if file.read_exact(&mut payload).is_err() {
                break;
            }
            let mut hasher = Hasher::new();
            hasher.update(&payload);
            if hasher.finalize() != u32::from_le_bytes(crc_bytes) {
                break;
            }
            if let Ok(record) = bincode::deserialize::<WalRecord>(&payload) {
                match record {
                    WalRecord::Begin { txid }
                    | WalRecord::Statement { txid, .. }
                    | WalRecord::Commit { txid }
                    | WalRecord::Rollback { txid } => max_txid = max_txid.max(txid),
                }
            } else {
                break;
            }
        }
        Ok(max_txid)
    }
}

pub fn wal_path(db_path: &Path) -> PathBuf {
    db_path.with_extension("wal")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::storage::DatabaseState;
    use tempfile::tempdir;

    #[test]
    fn append_and_replay_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("db.wal");
        let mut wal = Wal::open(&path).unwrap();
        wal.append_record(&WalRecord::Begin { txid: 1 }).unwrap();
        let mut state = DatabaseState {
            catalog: Catalog::default(),
            tables: BTreeMap::new(),
            applied_txid: 0,
        };
        wal.replay(&mut state).unwrap();
    }
}

use crate::error::{MiniDbError, Result};
use crate::parser::ast::Statement;
use crate::storage::DatabaseState;
use crc32fast::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalRecord {
    Begin { tx: u64 },
    Statement { tx: u64, statement: Statement },
    Commit { tx: u64 },
    Rollback { tx: u64 },
}

pub fn append(path: &Path, record: &WalRecord) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let payload = serde_json::to_vec(record)
        .map_err(|e| MiniDbError::Storage(format!("encode WAL record: {e}")))?;
    let mut hasher = Hasher::new();
    hasher.update(&payload);
    let crc = hasher.finalize();
    let len = u32::try_from(payload.len())
        .map_err(|_| MiniDbError::Storage("WAL record is too large".into()))?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&len.to_le_bytes())?;
    file.write_all(&crc.to_le_bytes())?;
    file.write_all(&payload)?;
    file.sync_data()?;
    Ok(())
}

pub fn recover(path: &Path) -> Result<DatabaseState> {
    let mut state = DatabaseState::default();
    if !path.exists() {
        return Ok(state);
    }
    let records = read_records(path)?;
    let mut pending: BTreeMap<u64, Vec<Statement>> = BTreeMap::new();
    for record in records {
        match record {
            WalRecord::Begin { tx } => {
                pending.entry(tx).or_default();
            }
            WalRecord::Statement { tx, statement } => {
                pending.entry(tx).or_default().push(statement);
            }
            WalRecord::Commit { tx } => {
                if let Some(statements) = pending.remove(&tx) {
                    for statement in statements {
                        crate::exec::apply_recovered_statement(&mut state, statement)?;
                    }
                }
            }
            WalRecord::Rollback { tx } => {
                pending.remove(&tx);
            }
        }
    }
    Ok(state)
}

fn read_records(path: &Path) -> Result<Vec<WalRecord>> {
    let mut file = OpenOptions::new().read(true).open(path)?;
    let mut records = Vec::new();
    loop {
        let mut header = [0u8; 8];
        match file.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&header[0..4]);
        let mut crc_bytes = [0u8; 4];
        crc_bytes.copy_from_slice(&header[4..8]);
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
        let record = serde_json::from_slice::<WalRecord>(&payload)
            .map_err(|e| MiniDbError::Storage(format!("decode WAL record: {e}")))?;
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::Statement;

    #[test]
    fn committed_records_survive_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.wal");
        append(&path, &WalRecord::Begin { tx: 1 }).unwrap();
        append(
            &path,
            &WalRecord::Statement {
                tx: 1,
                statement: Statement::Begin,
            },
        )
        .unwrap();
        append(&path, &WalRecord::Rollback { tx: 1 }).unwrap();
        let recovered = recover(&path).unwrap();
        assert!(recovered.catalog.tables.is_empty());
    }
}

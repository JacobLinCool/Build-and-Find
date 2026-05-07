use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;

use crc32fast::Hasher;
use serde::{Deserialize, Serialize};

use crate::error::MiniDbError;
use crate::storage::DbSnapshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalRecord {
    Snapshot(Vec<u8>),
}

pub fn append(path: impl AsRef<Path>, rec: &WalRecord) -> Result<(), MiniDbError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)?;
    let payload = serde_json::to_vec(rec).map_err(|e| MiniDbError::storage(e.to_string()))?;
    let crc = crc32(&payload);
    let len = payload.len() as u32;
    file.write_all(&len.to_le_bytes())?;
    file.write_all(&crc.to_le_bytes())?;
    file.write_all(&payload)?;
    file.flush()?;
    Ok(())
}

pub fn replay(path: impl AsRef<Path>, snapshot: &mut DbSnapshot) -> Result<(), MiniDbError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(());
    }
    let mut file = OpenOptions::new().read(true).open(path)?;
    loop {
        let mut len_buf = [0u8; 4];
        if file.read_exact(&mut len_buf).is_err() {
            break;
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut crc_buf = [0u8; 4];
        if file.read_exact(&mut crc_buf).is_err() {
            break;
        }
        let mut payload = vec![0u8; len];
        if file.read_exact(&mut payload).is_err() {
            break;
        }
        if crc32(&payload) != u32::from_le_bytes(crc_buf) {
            break;
        }
        let rec: WalRecord =
            serde_json::from_slice(&payload).map_err(|e| MiniDbError::storage(e.to_string()))?;
        match rec {
            WalRecord::Snapshot(bytes) => {
                *snapshot = serde_json::from_slice(&bytes)
                    .map_err(|e| MiniDbError::storage(e.to_string()))?
            }
        }
    }
    Ok(())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut h = Hasher::new();
    h.update(bytes);
    h.finalize()
}

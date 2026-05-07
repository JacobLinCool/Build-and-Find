use crate::{catalog::Catalog, error::Result};
use crc32fast::Hasher;
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Write},
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WalRecord {
    Snapshot(Catalog),
}

fn encode(record: &WalRecord) -> Result<String> {
    let payload = serde_json::to_string(record)?;
    let mut hasher = Hasher::new();
    hasher.update(payload.as_bytes());
    Ok(format!("{}|{}\n", hasher.finalize(), payload))
}

pub fn append(path: &Path, record: &WalRecord) -> Result<()> {
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(encode(record)?.as_bytes())?;
    f.sync_all()?;
    Ok(())
}

pub fn replay(path: &Path, mut catalog: Catalog) -> Result<Catalog> {
    if !path.exists() {
        return Ok(catalog);
    }
    let f = OpenOptions::new().read(true).open(path)?;
    for line in BufReader::new(f).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Some((crc, payload)) = line.split_once('|') else {
            break;
        };
        let mut hasher = Hasher::new();
        hasher.update(payload.as_bytes());
        if crc != hasher.finalize().to_string() {
            break;
        }
        let rec: WalRecord = serde_json::from_str(payload)?;
        match rec {
            WalRecord::Snapshot(c) => catalog = c,
        }
    }
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    #[test]
    fn append_and_replay() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.wal");
        let c = Catalog::default();
        append(&p, &WalRecord::Snapshot(c.clone())).unwrap();
        assert!(replay(&p, c).is_ok());
    }
}

use crate::{
    catalog::{IndexSchema, TableSchema},
    error::{MiniDbError, Result},
    types::Row,
};
use crc32fast::Hasher;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalOp {
    CreateTable {
        schema: TableSchema,
    },
    DropTable {
        name: String,
    },
    CreateIndex {
        index: IndexSchema,
    },
    DropIndex {
        name: String,
    },
    Insert {
        table: String,
        row: Row,
    },
    Update {
        table: String,
        row_id: u64,
        row: Row,
    },
    Delete {
        table: String,
        row_id: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum WalRecord {
    Begin,
    Op(WalOp),
    Commit,
}

pub struct Wal {
    path: PathBuf,
    file: File,
}

impl Wal {
    pub fn open(path: PathBuf) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)?;
        Ok(Self { path, file })
    }
    pub fn begin(&mut self) -> Result<()> {
        self.write(&WalRecord::Begin)
    }
    pub fn op(&mut self, op: &WalOp) -> Result<()> {
        self.write(&WalRecord::Op(op.clone()))
    }
    pub fn commit(&mut self) -> Result<()> {
        self.write(&WalRecord::Commit)?;
        self.file.sync_all()?;
        Ok(())
    }
    pub fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        Ok(())
    }
    fn write(&mut self, rec: &WalRecord) -> Result<()> {
        let payload = serde_json::to_vec(rec)?;
        let mut h = Hasher::new();
        h.update(&payload);
        let crc = h.finalize();
        self.file.write_all(&(payload.len() as u32).to_le_bytes())?;
        self.file.write_all(&crc.to_le_bytes())?;
        self.file.write_all(&payload)?;
        Ok(())
    }
    pub fn replay(&self) -> Result<Vec<Vec<WalOp>>> {
        let mut f = OpenOptions::new().read(true).open(&self.path)?;
        let mut txs = Vec::new();
        let mut cur: Option<Vec<WalOp>> = None;
        loop {
            let mut header = [0u8; 8];
            match f.read_exact(&mut header) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
            let len = u32::from_le_bytes(
                header[0..4]
                    .try_into()
                    .map_err(|_| MiniDbError::Storage("bad wal header".into()))?,
            ) as usize;
            let crc = u32::from_le_bytes(
                header[4..8]
                    .try_into()
                    .map_err(|_| MiniDbError::Storage("bad wal header".into()))?,
            );
            let mut payload = vec![0; len];
            if f.read_exact(&mut payload).is_err() {
                break;
            }
            let mut h = Hasher::new();
            h.update(&payload);
            if h.finalize() != crc {
                break;
            }
            match serde_json::from_slice::<WalRecord>(&payload)? {
                WalRecord::Begin => cur = Some(Vec::new()),
                WalRecord::Op(op) => {
                    if let Some(c) = &mut cur {
                        c.push(op);
                    }
                }
                WalRecord::Commit => {
                    if let Some(c) = cur.take() {
                        txs.push(c);
                    }
                }
            }
        }
        Ok(txs)
    }
}

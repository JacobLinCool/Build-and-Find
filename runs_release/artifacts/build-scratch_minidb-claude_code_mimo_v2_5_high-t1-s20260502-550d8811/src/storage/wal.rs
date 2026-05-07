use crate::error::{MiniDbError, Result};
use crc32fast::Hasher;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum WalOp {
    Begin {
        tx_id: u64,
    },
    Commit {
        tx_id: u64,
    },
    Rollback {
        tx_id: u64,
    },
    Insert {
        tx_id: u64,
        table: String,
        row_data: Vec<u8>,
        row_id: u64,
    },
    Update {
        tx_id: u64,
        table: String,
        row_id: u64,
        old_data: Vec<u8>,
        new_data: Vec<u8>,
    },
    Delete {
        tx_id: u64,
        table: String,
        row_id: u64,
    },
    CreateTable {
        tx_id: u64,
        table: String,
        schema_data: Vec<u8>,
    },
    DropTable {
        tx_id: u64,
        table: String,
    },
    CreateIndex {
        tx_id: u64,
        index_name: String,
        table: String,
        column: String,
    },
    DropIndex {
        tx_id: u64,
        index_name: String,
    },
}

#[derive(Debug, Clone)]
pub struct WalRecord {
    pub op: WalOp,
    pub lsn: u64,
}

fn encode_u64(val: u64) -> [u8; 8] {
    val.to_le_bytes()
}

fn decode_u64(data: &[u8]) -> u64 {
    u64::from_le_bytes(data.try_into().unwrap())
}

fn crc32_of(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

impl WalOp {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            WalOp::Begin { tx_id } => {
                buf.push(0x01);
                buf.extend_from_slice(&encode_u64(*tx_id));
            }
            WalOp::Commit { tx_id } => {
                buf.push(0x02);
                buf.extend_from_slice(&encode_u64(*tx_id));
            }
            WalOp::Rollback { tx_id } => {
                buf.push(0x03);
                buf.extend_from_slice(&encode_u64(*tx_id));
            }
            WalOp::Insert {
                tx_id,
                table,
                row_data,
                row_id,
            } => {
                buf.push(0x10);
                buf.extend_from_slice(&encode_u64(*tx_id));
                let tbytes = table.as_bytes();
                buf.extend_from_slice(&(tbytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(tbytes);
                buf.extend_from_slice(&encode_u64(*row_id));
                buf.extend_from_slice(&(row_data.len() as u32).to_le_bytes());
                buf.extend_from_slice(row_data);
            }
            WalOp::Update {
                tx_id,
                table,
                row_id,
                old_data,
                new_data,
            } => {
                buf.push(0x11);
                buf.extend_from_slice(&encode_u64(*tx_id));
                let tbytes = table.as_bytes();
                buf.extend_from_slice(&(tbytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(tbytes);
                buf.extend_from_slice(&encode_u64(*row_id));
                buf.extend_from_slice(&(old_data.len() as u32).to_le_bytes());
                buf.extend_from_slice(old_data);
                buf.extend_from_slice(&(new_data.len() as u32).to_le_bytes());
                buf.extend_from_slice(new_data);
            }
            WalOp::Delete {
                tx_id,
                table,
                row_id,
            } => {
                buf.push(0x12);
                buf.extend_from_slice(&encode_u64(*tx_id));
                let tbytes = table.as_bytes();
                buf.extend_from_slice(&(tbytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(tbytes);
                buf.extend_from_slice(&encode_u64(*row_id));
            }
            WalOp::CreateTable {
                tx_id,
                table,
                schema_data,
            } => {
                buf.push(0x20);
                buf.extend_from_slice(&encode_u64(*tx_id));
                let tbytes = table.as_bytes();
                buf.extend_from_slice(&(tbytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(tbytes);
                buf.extend_from_slice(&(schema_data.len() as u32).to_le_bytes());
                buf.extend_from_slice(schema_data);
            }
            WalOp::DropTable { tx_id, table } => {
                buf.push(0x21);
                buf.extend_from_slice(&encode_u64(*tx_id));
                let tbytes = table.as_bytes();
                buf.extend_from_slice(&(tbytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(tbytes);
            }
            WalOp::CreateIndex {
                tx_id,
                index_name,
                table,
                column,
            } => {
                buf.push(0x30);
                buf.extend_from_slice(&encode_u64(*tx_id));
                let ibytes = index_name.as_bytes();
                buf.extend_from_slice(&(ibytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(ibytes);
                let tbytes = table.as_bytes();
                buf.extend_from_slice(&(tbytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(tbytes);
                let cbytes = column.as_bytes();
                buf.extend_from_slice(&(cbytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(cbytes);
            }
            WalOp::DropIndex { tx_id, index_name } => {
                buf.push(0x31);
                buf.extend_from_slice(&encode_u64(*tx_id));
                let ibytes = index_name.as_bytes();
                buf.extend_from_slice(&(ibytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(ibytes);
            }
        }
        buf
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.is_empty() {
            return Err(MiniDbError::Storage("empty WAL op data".into()));
        }
        let mut pos;
        let read_u64 = |pos: &mut usize| -> Result<u64> {
            if *pos + 8 > data.len() {
                return Err(MiniDbError::Storage("truncated WAL record".into()));
            }
            let val = decode_u64(&data[*pos..*pos + 8]);
            *pos += 8;
            Ok(val)
        };
        let read_bytes = |pos: &mut usize| -> Result<Vec<u8>> {
            if *pos + 4 > data.len() {
                return Err(MiniDbError::Storage("truncated WAL record".into()));
            }
            let len = u32::from_le_bytes(data[*pos..*pos + 4].try_into().unwrap()) as usize;
            *pos += 4;
            if *pos + len > data.len() {
                return Err(MiniDbError::Storage("truncated WAL record".into()));
            }
            let bytes = data[*pos..*pos + len].to_vec();
            *pos += len;
            Ok(bytes)
        };

        let tag = data[0];
        pos = 1;

        match tag {
            0x01 => {
                let tx_id = read_u64(&mut pos)?;
                Ok(WalOp::Begin { tx_id })
            }
            0x02 => {
                let tx_id = read_u64(&mut pos)?;
                Ok(WalOp::Commit { tx_id })
            }
            0x03 => {
                let tx_id = read_u64(&mut pos)?;
                Ok(WalOp::Rollback { tx_id })
            }
            0x10 => {
                let tx_id = read_u64(&mut pos)?;
                let table = String::from_utf8(read_bytes(&mut pos)?)
                    .map_err(|e| MiniDbError::Storage(format!("invalid UTF-8: {}", e)))?;
                let row_id = read_u64(&mut pos)?;
                let row_data = read_bytes(&mut pos)?;
                Ok(WalOp::Insert {
                    tx_id,
                    table,
                    row_data,
                    row_id,
                })
            }
            0x11 => {
                let tx_id = read_u64(&mut pos)?;
                let table = String::from_utf8(read_bytes(&mut pos)?)
                    .map_err(|e| MiniDbError::Storage(format!("invalid UTF-8: {}", e)))?;
                let row_id = read_u64(&mut pos)?;
                let old_data = read_bytes(&mut pos)?;
                let new_data = read_bytes(&mut pos)?;
                Ok(WalOp::Update {
                    tx_id,
                    table,
                    row_id,
                    old_data,
                    new_data,
                })
            }
            0x12 => {
                let tx_id = read_u64(&mut pos)?;
                let table = String::from_utf8(read_bytes(&mut pos)?)
                    .map_err(|e| MiniDbError::Storage(format!("invalid UTF-8: {}", e)))?;
                let row_id = read_u64(&mut pos)?;
                Ok(WalOp::Delete {
                    tx_id,
                    table,
                    row_id,
                })
            }
            0x20 => {
                let tx_id = read_u64(&mut pos)?;
                let table = String::from_utf8(read_bytes(&mut pos)?)
                    .map_err(|e| MiniDbError::Storage(format!("invalid UTF-8: {}", e)))?;
                let schema_data = read_bytes(&mut pos)?;
                Ok(WalOp::CreateTable {
                    tx_id,
                    table,
                    schema_data,
                })
            }
            0x21 => {
                let tx_id = read_u64(&mut pos)?;
                let table = String::from_utf8(read_bytes(&mut pos)?)
                    .map_err(|e| MiniDbError::Storage(format!("invalid UTF-8: {}", e)))?;
                Ok(WalOp::DropTable { tx_id, table })
            }
            0x30 => {
                let tx_id = read_u64(&mut pos)?;
                let index_name = String::from_utf8(read_bytes(&mut pos)?)
                    .map_err(|e| MiniDbError::Storage(format!("invalid UTF-8: {}", e)))?;
                let table = String::from_utf8(read_bytes(&mut pos)?)
                    .map_err(|e| MiniDbError::Storage(format!("invalid UTF-8: {}", e)))?;
                let column = String::from_utf8(read_bytes(&mut pos)?)
                    .map_err(|e| MiniDbError::Storage(format!("invalid UTF-8: {}", e)))?;
                Ok(WalOp::CreateIndex {
                    tx_id,
                    index_name,
                    table,
                    column,
                })
            }
            0x31 => {
                let tx_id = read_u64(&mut pos)?;
                let index_name = String::from_utf8(read_bytes(&mut pos)?)
                    .map_err(|e| MiniDbError::Storage(format!("invalid UTF-8: {}", e)))?;
                Ok(WalOp::DropIndex { tx_id, index_name })
            }
            _ => Err(MiniDbError::Storage(format!(
                "unknown WAL op tag: 0x{:02x}",
                tag
            ))),
        }
    }
}

pub struct Wal {
    file: File,
    next_lsn: u64,
    path: String,
}

impl Wal {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(path)?;

        // Scan existing records to find next_lsn
        let mut next_lsn = 0u64;
        {
            let mut reader_file = OpenOptions::new().read(true).open(path)?;
            loop {
                match Self::read_record_from(&mut reader_file) {
                    Ok(Some((_record, _size))) => {
                        next_lsn += 1;
                    }
                    _ => break,
                }
            }
        }

        log::info!(
            "Opened WAL: {} existing records, next_lsn={}",
            next_lsn,
            next_lsn
        );

        Ok(Wal {
            file,
            next_lsn,
            path: path.to_string_lossy().to_string(),
        })
    }

    fn read_record_from(file: &mut File) -> Result<Option<(WalRecord, usize)>> {
        let mut size_buf = [0u8; 4];
        match file.read_exact(&mut size_buf) {
            Ok(()) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(MiniDbError::Io(e.to_string())),
        }
        let record_len = u32::from_le_bytes(size_buf) as usize;

        if record_len < 4 {
            return Err(MiniDbError::Storage("invalid WAL record length".into()));
        }

        let mut record_buf = vec![0u8; record_len];
        file.read_exact(&mut record_buf)?;

        let stored_crc =
            u32::from_le_bytes(record_buf[record_len - 4..record_len].try_into().unwrap());
        let data = &record_buf[..record_len - 4];
        let computed_crc = crc32_of(data);

        if stored_crc != computed_crc {
            log::warn!(
                "WAL CRC mismatch: stored={:08x} computed={:08x}",
                stored_crc,
                computed_crc
            );
            return Err(MiniDbError::Storage(
                "WAL CRC mismatch - torn write detected".into(),
            ));
        }

        let op = WalOp::decode(data)?;
        let total_size = 4 + record_len;

        // Read the LSN from the record
        // For simplicity, we track lsn positionally
        Ok(Some((WalRecord { op, lsn: 0 }, total_size)))
    }

    pub fn append(&mut self, op: WalOp) -> Result<WalRecord> {
        let data = op.encode();
        let crc = crc32_of(&data);
        let record_len = data.len() as u32 + 4; // data + crc32

        // Write: [4-byte total_len][data][4-byte crc32]
        let len_buf = record_len.to_le_bytes();
        self.file.write_all(&len_buf)?;
        self.file.write_all(&data)?;
        self.file.write_all(&crc.to_le_bytes())?;
        self.file.flush()?;

        let lsn = self.next_lsn;
        self.next_lsn += 1;

        log::debug!("WAL append lsn={}: {:?}", lsn, std::mem::discriminant(&op));

        Ok(WalRecord { op, lsn })
    }

    pub fn read_all_records(&self) -> Result<Vec<WalRecord>> {
        let mut file = OpenOptions::new().read(true).open(&self.path)?;
        let mut records = Vec::new();
        let mut lsn = 0u64;
        loop {
            match Self::read_record_from(&mut file)? {
                None => break,
                Some((mut record, _)) => {
                    record.lsn = lsn;
                    lsn += 1;
                    records.push(record);
                }
            }
        }
        Ok(records)
    }

    pub fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.flush()?;
        self.next_lsn = 0;
        log::info!("WAL truncated");
        Ok(())
    }

    pub fn next_lsn(&self) -> u64 {
        self.next_lsn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_wal_write_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");

        {
            let mut wal = Wal::open(&path).unwrap();
            wal.append(WalOp::Begin { tx_id: 1 }).unwrap();
            wal.append(WalOp::Insert {
                tx_id: 1,
                table: "users".into(),
                row_data: vec![1, 2, 3],
                row_id: 0,
            })
            .unwrap();
            wal.append(WalOp::Commit { tx_id: 1 }).unwrap();
        }

        let wal = Wal::open(&path).unwrap();
        let records = wal.read_all_records().unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].op, WalOp::Begin { tx_id: 1 });
        assert_eq!(records[2].op, WalOp::Commit { tx_id: 1 });
    }

    #[test]
    fn test_wal_truncate() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");

        let mut wal = Wal::open(&path).unwrap();
        wal.append(WalOp::Begin { tx_id: 1 }).unwrap();
        wal.truncate().unwrap();
        let records = wal.read_all_records().unwrap();
        assert_eq!(records.len(), 0);
    }

    #[test]
    fn test_wal_encode_decode_roundtrip() {
        let ops = vec![
            WalOp::Begin { tx_id: 42 },
            WalOp::Insert {
                tx_id: 42,
                table: "t".into(),
                row_data: vec![10, 20],
                row_id: 5,
            },
            WalOp::Update {
                tx_id: 42,
                table: "t".into(),
                row_id: 5,
                old_data: vec![10, 20],
                new_data: vec![30, 40],
            },
            WalOp::Delete {
                tx_id: 42,
                table: "t".into(),
                row_id: 5,
            },
            WalOp::Commit { tx_id: 42 },
            WalOp::Rollback { tx_id: 7 },
            WalOp::CreateTable {
                tx_id: 1,
                table: "test".into(),
                schema_data: vec![1, 2, 3],
            },
            WalOp::DropTable {
                tx_id: 1,
                table: "test".into(),
            },
            WalOp::CreateIndex {
                tx_id: 1,
                index_name: "idx".into(),
                table: "t".into(),
                column: "c".into(),
            },
            WalOp::DropIndex {
                tx_id: 1,
                index_name: "idx".into(),
            },
        ];

        for op in &ops {
            let encoded = op.encode();
            let decoded = WalOp::decode(&encoded).unwrap();
            assert_eq!(*op, decoded);
        }
    }
}

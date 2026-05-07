use crate::error::{MiniDbError, Result};
use crc32fast::Hasher;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::Path;

const RECORD_HEADER_SIZE: usize = 12; // 4 len + 4 crc + 4 type

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum WalRecordType {
    PageWrite = 1,
    Commit = 2,
    Checkpoint = 3,
}

impl WalRecordType {
    fn from_u32(v: u32) -> Result<Self> {
        match v {
            1 => Ok(Self::PageWrite),
            2 => Ok(Self::Commit),
            3 => Ok(Self::Checkpoint),
            _ => Err(MiniDbError::Wal(format!("unknown record type: {v}"))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WalRecord {
    pub record_type: WalRecordType,
    pub data: Vec<u8>,
}

impl WalRecord {
    pub fn page_write(page_id: u64, page_data: &[u8]) -> Self {
        let mut data = Vec::with_capacity(8 + page_data.len());
        data.extend_from_slice(&page_id.to_le_bytes());
        data.extend_from_slice(page_data);
        Self {
            record_type: WalRecordType::PageWrite,
            data,
        }
    }

    pub fn commit(txn_id: u64) -> Self {
        Self {
            record_type: WalRecordType::Commit,
            data: txn_id.to_le_bytes().to_vec(),
        }
    }

    pub fn checkpoint() -> Self {
        Self {
            record_type: WalRecordType::Checkpoint,
            data: Vec::new(),
        }
    }

    fn encoded_len(&self) -> u32 {
        (RECORD_HEADER_SIZE + self.data.len()) as u32
    }

    fn compute_crc(&self) -> u32 {
        let mut hasher = Hasher::new();
        hasher.update(&(self.record_type as u32).to_le_bytes());
        hasher.update(&self.data);
        hasher.finalize()
    }
}

pub struct Wal {
    writer: BufWriter<File>,
    offset: u64,
}

impl Wal {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let offset = file.metadata()?.len();
        Ok(Self {
            writer: BufWriter::new(file),
            offset,
        })
    }

    pub fn create(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        Ok(Self {
            writer: BufWriter::new(file),
            offset: 0,
        })
    }

    pub fn append(&mut self, record: &WalRecord) -> Result<u64> {
        let start_offset = self.offset;
        let len = record.encoded_len();
        let crc = record.compute_crc();
        let rtype = record.record_type as u32;

        self.writer.write_all(&len.to_le_bytes())?;
        self.writer.write_all(&crc.to_le_bytes())?;
        self.writer.write_all(&rtype.to_le_bytes())?;
        self.writer.write_all(&record.data)?;
        self.writer.flush()?;

        self.offset += len as u64;
        Ok(start_offset)
    }

    pub fn flush_and_sync(&mut self) -> Result<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_data()?;
        Ok(())
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }
}

pub struct WalReader {
    data: Vec<u8>,
    pos: usize,
}

impl WalReader {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Ok(Self { data, pos: 0 })
    }

    pub fn read_all(&mut self) -> Result<Vec<WalRecord>> {
        let mut records = Vec::new();
        while self.pos + RECORD_HEADER_SIZE <= self.data.len() {
            let len =
                u32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap()) as usize;
            if len < RECORD_HEADER_SIZE || self.pos + len > self.data.len() {
                break;
            }
            let crc_stored =
                u32::from_le_bytes(self.data[self.pos + 4..self.pos + 8].try_into().unwrap());
            let rtype =
                u32::from_le_bytes(self.data[self.pos + 8..self.pos + 12].try_into().unwrap());
            let payload = &self.data[self.pos + RECORD_HEADER_SIZE..self.pos + len];

            let record_type = WalRecordType::from_u32(rtype)?;
            let mut hasher = Hasher::new();
            hasher.update(&rtype.to_le_bytes());
            hasher.update(payload);
            let crc_computed = hasher.finalize();

            if crc_stored != crc_computed {
                log::warn!(
                    "WAL CRC mismatch at offset {}: stored={:#010x} computed={:#010x}, stopping replay",
                    self.pos, crc_stored, crc_computed
                );
                break;
            }

            records.push(WalRecord {
                record_type,
                data: payload.to_vec(),
            });
            self.pos += len;
        }
        Ok(records)
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
            let mut wal = Wal::create(&path).unwrap();
            let r1 = WalRecord::page_write(0, &[42u8; 100]);
            wal.append(&r1).unwrap();
            let r2 = WalRecord::commit(1);
            wal.append(&r2).unwrap();
            wal.flush_and_sync().unwrap();
        }
        {
            let mut reader = WalReader::open(&path).unwrap();
            let records = reader.read_all().unwrap();
            assert_eq!(records.len(), 2);
            assert_eq!(records[0].record_type, WalRecordType::PageWrite);
            assert_eq!(records[0].data[0..8], 0u64.to_le_bytes());
            assert_eq!(records[0].data[8], 42);
            assert_eq!(records[1].record_type, WalRecordType::Commit);
        }
    }

    #[test]
    fn test_wal_crc_detection() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wal");
        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append(&WalRecord::commit(1)).unwrap();
            wal.flush_and_sync().unwrap();
        }
        // Corrupt the CRC field (bytes 4-7)
        let mut data = std::fs::read(&path).unwrap();
        data[4] ^= 0xFF;
        std::fs::write(&path, &data).unwrap();
        let mut reader = WalReader::open(&path).unwrap();
        let records = reader.read_all().unwrap();
        assert!(records.is_empty());
    }
}

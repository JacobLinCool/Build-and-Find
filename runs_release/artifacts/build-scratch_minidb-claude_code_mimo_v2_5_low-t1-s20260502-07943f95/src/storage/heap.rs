use super::RowId;
use crate::error::{MiniDbError, Result};
use log::debug;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

pub struct HeapFile {
    file: File,
    path: String,
    row_offsets: Vec<(RowId, u64, u32)>, // (row_id, file_offset, length)
    next_row_id: u64,
}

impl HeapFile {
    pub fn open(path: &str) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)?;
        let mut heap = Self {
            file,
            path: path.to_string(),
            row_offsets: Vec::new(),
            next_row_id: 0,
        };
        heap.load_index()?;
        Ok(heap)
    }

    fn load_index(&mut self) -> Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut buf = Vec::new();
        self.file.read_to_end(&mut buf)?;
        let mut pos = 0;
        while pos + 8 <= buf.len() {
            // header: row_id(8) + length(4) + data(length) + deleted(1)
            let row_id = u64::from_be_bytes(
                buf[pos..pos + 8]
                    .try_into()
                    .map_err(|_| MiniDbError::Storage("Invalid row_id".into()))?,
            );
            pos += 8;
            if pos + 4 > buf.len() {
                break;
            }
            let length = u32::from_be_bytes(
                buf[pos..pos + 4]
                    .try_into()
                    .map_err(|_| MiniDbError::Storage("Invalid length".into()))?,
            );
            pos += 4;
            if pos + length as usize + 1 > buf.len() {
                break;
            }
            let deleted = buf[pos + length as usize];
            if deleted == 0 {
                self.row_offsets.push((RowId(row_id), pos as u64, length));
            }
            pos += length as usize + 1; // data + deleted flag
            self.next_row_id = self.next_row_id.max(row_id + 1);
        }
        Ok(())
    }

    pub fn add_row(&mut self, _table: &str, data: &[u8]) -> Result<RowId> {
        let row_id = RowId(self.next_row_id);
        self.next_row_id += 1;

        // Seek to end
        self.file.seek(SeekFrom::End(0))?;
        let offset = self.file.stream_position()?;

        // Write header
        self.file.write_all(&row_id.0.to_be_bytes())?;
        self.file.write_all(&(data.len() as u32).to_be_bytes())?;
        // Write data
        self.file.write_all(data)?;
        // Write deleted flag (0 = not deleted)
        self.file.write_all(&[0u8])?;
        self.file.sync_all()?;

        self.row_offsets
            .push((row_id, offset + 12, data.len() as u32));
        debug!("Added row {:?} at offset {}", row_id, offset);
        Ok(row_id)
    }

    pub fn get_row(&self, row_id: RowId) -> Result<Vec<u8>> {
        if let Some(&(_, offset, length)) = self.row_offsets.iter().find(|(id, _, _)| *id == row_id)
        {
            let mut buf = vec![0u8; length as usize];
            let mut file = File::open(&self.path)?;
            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(&mut buf)?;
            Ok(buf)
        } else {
            Err(MiniDbError::Storage(format!("Row {:?} not found", row_id)))
        }
    }

    pub fn update_row(&mut self, row_id: RowId, new_data: &[u8]) -> Result<()> {
        if let Some(pos) = self.row_offsets.iter().position(|(id, _, _)| *id == row_id) {
            // Mark old row as deleted
            let old_offset = self.row_offsets[pos].1;
            let old_length = self.row_offsets[pos].2;
            let deleted_offset = old_offset + old_length as u64;
            self.file.seek(SeekFrom::Start(deleted_offset))?;
            self.file.write_all(&[1u8])?; // deleted

            // Write new row at end
            self.file.seek(SeekFrom::End(0))?;
            let new_offset = self.file.stream_position()?;
            self.file.write_all(&row_id.0.to_be_bytes())?;
            self.file
                .write_all(&(new_data.len() as u32).to_be_bytes())?;
            self.file.write_all(new_data)?;
            self.file.write_all(&[0u8])?; // not deleted
            self.file.sync_all()?;

            self.row_offsets[pos] = (row_id, new_offset + 12, new_data.len() as u32);
            Ok(())
        } else {
            Err(MiniDbError::Storage(format!("Row {:?} not found", row_id)))
        }
    }

    pub fn delete_row(&mut self, row_id: RowId) -> Result<()> {
        if let Some(pos) = self.row_offsets.iter().position(|(id, _, _)| *id == row_id) {
            let offset = self.row_offsets[pos].1;
            let length = self.row_offsets[pos].2;
            let deleted_offset = offset + length as u64;
            self.file.seek(SeekFrom::Start(deleted_offset))?;
            self.file.write_all(&[1u8])?;
            self.row_offsets.remove(pos);
            Ok(())
        } else {
            Ok(())
        }
    }

    pub fn all_row_ids(&self) -> Vec<RowId> {
        self.row_offsets.iter().map(|(id, _, _)| *id).collect()
    }

    pub fn flush(&mut self) -> Result<()> {
        self.file.sync_all()?;
        Ok(())
    }

    pub fn row_count(&self) -> usize {
        self.row_offsets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile;

    #[test]
    fn test_heap_add_and_get_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.heap");
        let path_str = path.to_str().unwrap();
        {
            let mut heap = HeapFile::open(path_str).unwrap();
            let row_id = heap.add_row("users", b"hello").unwrap();
            let data = heap.get_row(row_id).unwrap();
            assert_eq!(data, b"hello");
        }
        // Reopen and verify
        let heap = HeapFile::open(path_str).unwrap();
        let ids = heap.all_row_ids();
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn test_heap_multiple_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.heap");
        let path_str = path.to_str().unwrap();
        let mut heap = HeapFile::open(path_str).unwrap();
        let id1 = heap.add_row("t", b"row1").unwrap();
        let id2 = heap.add_row("t", b"row2").unwrap();
        assert_ne!(id1, id2);
        assert_eq!(heap.get_row(id1).unwrap(), b"row1");
        assert_eq!(heap.get_row(id2).unwrap(), b"row2");
    }

    #[test]
    fn test_heap_update_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.heap");
        let path_str = path.to_str().unwrap();
        let mut heap = HeapFile::open(path_str).unwrap();
        let id = heap.add_row("t", b"old").unwrap();
        heap.update_row(id, b"new").unwrap();
        assert_eq!(heap.get_row(id).unwrap(), b"new");
    }
}

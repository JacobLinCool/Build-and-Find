use crate::error::{MiniDbError, Result};
use crate::storage::page::{Page, PageId, PAGE_SIZE};
use crate::types::Value;

/// Heap file page layout:
///   [0..8]:   page_id (u64)
///   [8..12]:  row_count (u32)
///   [12..16]: data_end (u32) - offset where row data area ends (from page end)
///   [16..20]: slot_count (u32)
///   [20..]:   slot directory (each entry: 4 bytes offset + 2 bytes length = 6 bytes)
///   ...
///   [..PAGE_SIZE]: row data packed from end of page growing backwards

const HEADER_SIZE: usize = 20;
const SLOT_ENTRY_SIZE: usize = 6; // offset(u32) + len(u16)

pub struct HeapFile {
    root_page: PageId,
}

impl HeapFile {
    pub fn new(root_page: PageId) -> Self {
        Self { root_page }
    }

    pub fn root_page(&self) -> PageId {
        self.root_page
    }

    /// Initialize a heap page
    pub fn init_page(page: &mut Page) {
        page.write_u64(0, page.id.0);
        page.write_u32(8, 0); // row_count
        page.write_u32(12, 0); // data_end (bytes used from end of page)
        page.write_u32(16, 0); // slot_count
    }

    /// Serialize a row of values to bytes
    pub fn serialize_row(row: &[Value]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(row.len() as u32).to_le_bytes());
        for val in row {
            val.serialize_to(&mut buf);
        }
        buf
    }

    /// Deserialize a row of values from bytes
    pub fn deserialize_row(data: &[u8]) -> Result<Vec<Value>> {
        if data.len() < 4 {
            return Err(MiniDbError::Storage("row data too short".into()));
        }
        let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        let mut values = Vec::with_capacity(count);
        let mut pos = 4;
        for _ in 0..count {
            let (val, consumed) = Value::deserialize_from(&data[pos..])?;
            values.push(val);
            pos += consumed;
        }
        Ok(values)
    }

    /// Insert a row into the heap, returns (page_id, slot_index, row_id)
    pub fn insert_row(page: &mut Page, row: &[Value], row_id: u64) -> Result<(PageId, u32, u64)> {
        let serialized = Self::serialize_row(row);
        let row_len = serialized.len();

        let row_count = page.read_u32(8);
        let data_end = page.read_u32(12) as usize; // bytes used from end of page
        let slot_count = page.read_u32(16);

        // Check slot directory won't overlap with data area
        let new_slot_count = slot_count + 1;
        let slot_dir_end = HEADER_SIZE + new_slot_count as usize * SLOT_ENTRY_SIZE;

        // Row data is stored from PAGE_SIZE backwards
        let row_data_start = PAGE_SIZE - data_end - row_len;

        if row_data_start < slot_dir_end {
            return Err(MiniDbError::Storage("page is full".into()));
        }

        // Write row data from end of page
        page.write_bytes(row_data_start, &serialized);

        // Write slot entry (store absolute offset)
        let slot_offset = HEADER_SIZE + slot_count as usize * SLOT_ENTRY_SIZE;
        page.write_u32(slot_offset, row_data_start as u32);
        page.write_u16(slot_offset + 4, row_len as u16);

        // Update header
        page.write_u32(8, row_count + 1);
        page.write_u32(12, (data_end + row_len) as u32);
        page.write_u32(16, new_slot_count);

        Ok((page.id, slot_count, row_id))
    }

    /// Read all rows from a page
    pub fn scan_page(page: &Page) -> Result<Vec<(u32, Vec<Value>)>> {
        let slot_count = page.read_u32(16);
        let mut rows = Vec::new();

        for slot_idx in 0..slot_count {
            let slot_offset = HEADER_SIZE + slot_idx as usize * SLOT_ENTRY_SIZE;
            let data_offset = page.read_u32(slot_offset) as usize;
            let data_len = page.read_u16(slot_offset + 4) as usize;

            if data_len == 0 {
                continue; // deleted slot
            }

            if data_offset + data_len > PAGE_SIZE {
                return Err(MiniDbError::Storage(format!(
                    "invalid slot data: offset {} + len {} > PAGE_SIZE",
                    data_offset, data_len
                )));
            }

            let data = page.read_bytes(data_offset, data_len);
            let values = Self::deserialize_row(data)?;
            rows.push((slot_idx, values));
        }

        Ok(rows)
    }

    /// Delete a row by slot index (mark as deleted by setting length to 0)
    pub fn delete_row(page: &mut Page, slot_idx: u32) -> Result<()> {
        let slot_count = page.read_u32(16);
        if slot_idx >= slot_count {
            return Err(MiniDbError::Storage("slot index out of bounds".into()));
        }

        let slot_offset = HEADER_SIZE + slot_idx as usize * SLOT_ENTRY_SIZE;
        let data_len = page.read_u16(slot_offset + 4);

        if data_len == 0 {
            return Err(MiniDbError::Storage("row already deleted".into()));
        }

        // Mark as deleted
        page.write_u16(slot_offset + 4, 0);

        // Decrement row count
        let row_count = page.read_u32(8);
        page.write_u32(8, row_count - 1);

        Ok(())
    }

    /// Update a row in place, or relocate if the new row is larger
    pub fn update_row(page: &mut Page, slot_idx: u32, row: &[Value]) -> Result<()> {
        let slot_count = page.read_u32(16);
        if slot_idx >= slot_count {
            return Err(MiniDbError::Storage("slot index out of bounds".into()));
        }

        let slot_offset = HEADER_SIZE + slot_idx as usize * SLOT_ENTRY_SIZE;
        let data_offset = page.read_u32(slot_offset) as usize;
        let data_len = page.read_u16(slot_offset + 4) as usize;

        if data_len == 0 {
            return Err(MiniDbError::Storage("row already deleted".into()));
        }

        let serialized = Self::serialize_row(row);

        if serialized.len() <= data_len {
            // Fits in place
            page.write_bytes(data_offset, &serialized);
            page.write_u16(slot_offset + 4, serialized.len() as u16);
        } else {
            // Relocate: mark old slot as deleted, append new data
            let data_end = page.read_u32(12) as usize;
            let slot_dir_end = HEADER_SIZE + slot_count as usize * SLOT_ENTRY_SIZE;
            let new_data_start = PAGE_SIZE - data_end - serialized.len();

            if new_data_start < slot_dir_end {
                return Err(MiniDbError::Storage("page is full".into()));
            }

            // Write new data at the new location
            page.write_bytes(new_data_start, &serialized);

            // Update slot entry to point to new location
            page.write_u32(slot_offset, new_data_start as u32);
            page.write_u16(slot_offset + 4, serialized.len() as u16);

            // Update data_end (old space is leaked but acceptable for now)
            page.write_u32(12, (data_end + serialized.len()) as u32);
        }

        Ok(())
    }
}

impl Value {
    pub fn serialize_to(&self, buf: &mut Vec<u8>) {
        match self {
            Value::Null => {
                buf.push(0);
            }
            Value::Int(i) => {
                buf.push(1);
                buf.extend_from_slice(&i.to_le_bytes());
            }
            Value::Real(r) => {
                buf.push(2);
                buf.extend_from_slice(&r.to_le_bytes());
            }
            Value::Text(s) => {
                buf.push(3);
                let bytes = s.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(bytes);
            }
            Value::Bool(b) => {
                buf.push(4);
                buf.push(if *b { 1 } else { 0 });
            }
        }
    }

    pub fn deserialize_from(data: &[u8]) -> Result<(Value, usize)> {
        if data.is_empty() {
            return Err(MiniDbError::Storage("empty value data".into()));
        }
        match data[0] {
            0 => Ok((Value::Null, 1)),
            1 => {
                if data.len() < 9 {
                    return Err(MiniDbError::Storage("truncated int value".into()));
                }
                let val = i64::from_le_bytes(data[1..9].try_into().unwrap());
                Ok((Value::Int(val), 9))
            }
            2 => {
                if data.len() < 9 {
                    return Err(MiniDbError::Storage("truncated real value".into()));
                }
                let val = f64::from_le_bytes(data[1..9].try_into().unwrap());
                Ok((Value::Real(val), 9))
            }
            3 => {
                if data.len() < 5 {
                    return Err(MiniDbError::Storage("truncated text value".into()));
                }
                let len = u32::from_le_bytes(data[1..5].try_into().unwrap()) as usize;
                if data.len() < 5 + len {
                    return Err(MiniDbError::Storage("truncated text value".into()));
                }
                let s = String::from_utf8(data[5..5 + len].to_vec())
                    .map_err(|e| MiniDbError::Storage(format!("invalid UTF-8: {}", e)))?;
                Ok((Value::Text(s), 5 + len))
            }
            4 => {
                if data.len() < 2 {
                    return Err(MiniDbError::Storage("truncated bool value".into()));
                }
                Ok((Value::Bool(data[1] != 0), 2))
            }
            other => Err(MiniDbError::Storage(format!(
                "unknown value type tag: {}",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_row() {
        let row = vec![
            Value::Int(42),
            Value::Text("hello".into()),
            Value::Real(3.14),
            Value::Bool(true),
            Value::Null,
        ];
        let serialized = HeapFile::serialize_row(&row);
        let deserialized = HeapFile::deserialize_row(&serialized).unwrap();
        assert_eq!(row, deserialized);
    }

    #[test]
    fn test_heap_insert_and_scan() {
        let mut page = Page::new(PageId(0));
        HeapFile::init_page(&mut page);

        let row1 = vec![Value::Int(1), Value::Text("Alice".into())];
        let row2 = vec![Value::Int(2), Value::Text("Bob".into())];

        HeapFile::insert_row(&mut page, &row1, 1).unwrap();
        HeapFile::insert_row(&mut page, &row2, 2).unwrap();

        let rows = HeapFile::scan_page(&page).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, row1);
        assert_eq!(rows[1].1, row2);
    }

    #[test]
    fn test_heap_delete() {
        let mut page = Page::new(PageId(0));
        HeapFile::init_page(&mut page);

        let row = vec![Value::Int(1), Value::Text("test".into())];
        HeapFile::insert_row(&mut page, &row, 1).unwrap();

        HeapFile::delete_row(&mut page, 0).unwrap();

        let rows = HeapFile::scan_page(&page).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn test_heap_update() {
        let mut page = Page::new(PageId(0));
        HeapFile::init_page(&mut page);

        let row = vec![Value::Int(1), Value::Text("old".into())];
        HeapFile::insert_row(&mut page, &row, 1).unwrap();

        let new_row = vec![Value::Int(1), Value::Text("new".into())];
        HeapFile::update_row(&mut page, 0, &new_row).unwrap();

        let rows = HeapFile::scan_page(&page).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, new_row);
    }
}

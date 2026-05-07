use crate::error::{MiniDbError, Result};
use crate::storage::page::{PageId, PageManager, PAGE_SIZE};
use crate::types::{ColumnDef, Row, Value};
use std::io::Write;

/// Heap file: stores rows in slotted pages.
/// Page layout:
///   [2 bytes: slot_count] [2 bytes: free_offset] [slot_directory...] [row_data...]
/// Each slot: [2 bytes: offset] [2 bytes: length] (0 offset = deleted)
/// Row serialization: [4 bytes: value_count] for each value: [1 byte: tag] [data...]

const HEADER_SIZE: usize = 4;
const SLOT_SIZE: usize = 4;

pub struct HeapFile {
    table_name: String,
}

impl HeapFile {
    pub fn new(table_name: &str) -> Self {
        Self {
            table_name: table_name.to_string(),
        }
    }

    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    pub fn alloc_page(&self, pm: &mut PageManager) -> Result<PageId> {
        let pid = pm.alloc_page()?;
        let mut page = [0u8; PAGE_SIZE];
        // Initialize: slot_count=0, free_offset=PAGE_SIZE
        page[0..2].copy_from_slice(&0u16.to_le_bytes());
        page[2..4].copy_from_slice(&(PAGE_SIZE as u16).to_le_bytes());
        pm.write_page(pid, &page)?;
        Ok(pid)
    }

    pub fn insert_row(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        row: &Row,
        columns: &[ColumnDef],
    ) -> Result<u16> {
        let mut page = pm.read_page(page_id)?;
        let slot_count = u16::from_le_bytes(page[0..2].try_into().unwrap());
        let mut free_offset = u16::from_le_bytes(page[2..4].try_into().unwrap()) as usize;

        let serialized = serialize_row(row, columns);
        let row_len = serialized.len();

        let new_free = free_offset - row_len;
        let slots_end = HEADER_SIZE + (slot_count as usize + 1) * SLOT_SIZE;

        if new_free < slots_end {
            return Err(MiniDbError::Storage("page full".to_string()));
        }

        free_offset = new_free;
        page[free_offset..free_offset + row_len].copy_from_slice(&serialized);

        let slot_offset = HEADER_SIZE + slot_count as usize * SLOT_SIZE;
        page[slot_offset..slot_offset + 2].copy_from_slice(&(free_offset as u16).to_le_bytes());
        page[slot_offset + 2..slot_offset + 4].copy_from_slice(&(row_len as u16).to_le_bytes());

        let new_count = slot_count + 1;
        page[0..2].copy_from_slice(&new_count.to_le_bytes());
        page[2..4].copy_from_slice(&(free_offset as u16).to_le_bytes());

        pm.write_page(page_id, &page)?;
        Ok(slot_count)
    }

    pub fn scan_page(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        columns: &[ColumnDef],
    ) -> Result<Vec<(u16, Row)>> {
        let page = pm.read_page(page_id)?;
        let slot_count = u16::from_le_bytes(page[0..2].try_into().unwrap());
        let mut rows = Vec::new();

        for i in 0..slot_count {
            let slot_pos = HEADER_SIZE + i as usize * SLOT_SIZE;
            let offset = u16::from_le_bytes(page[slot_pos..slot_pos + 2].try_into().unwrap());
            let length = u16::from_le_bytes(page[slot_pos + 2..slot_pos + 4].try_into().unwrap());

            if offset == 0 || length == 0 {
                continue;
            }

            let data = &page[offset as usize..offset as usize + length as usize];
            let row = deserialize_row(data, columns)?;
            rows.push((i, row));
        }
        Ok(rows)
    }

    pub fn delete_row(&self, pm: &mut PageManager, page_id: PageId, slot_id: u16) -> Result<()> {
        let mut page = pm.read_page(page_id)?;
        let slot_count = u16::from_le_bytes(page[0..2].try_into().unwrap());
        if slot_id >= slot_count {
            return Err(MiniDbError::Storage("slot out of range".to_string()));
        }
        let slot_pos = HEADER_SIZE + slot_id as usize * SLOT_SIZE;
        page[slot_pos..slot_pos + 2].copy_from_slice(&0u16.to_le_bytes());
        page[slot_pos + 2..slot_pos + 4].copy_from_slice(&0u16.to_le_bytes());
        pm.write_page(page_id, &page)?;
        Ok(())
    }

    pub fn update_row(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        slot_id: u16,
        row: &Row,
        columns: &[ColumnDef],
    ) -> Result<()> {
        // Delete old, insert new on same page (simplistic approach)
        self.delete_row(pm, page_id, slot_id)?;
        let mut page = pm.read_page(page_id)?;
        let slot_count = u16::from_le_bytes(page[0..2].try_into().unwrap());
        let mut free_offset = u16::from_le_bytes(page[2..4].try_into().unwrap()) as usize;

        let serialized = serialize_row(row, columns);
        let row_len = serialized.len();
        let new_free = free_offset - row_len;
        let slots_end = HEADER_SIZE + (slot_count as usize) * SLOT_SIZE;

        if new_free < slots_end {
            return Err(MiniDbError::Storage("page full on update".to_string()));
        }

        free_offset = new_free;
        page[free_offset..free_offset + row_len].copy_from_slice(&serialized);

        // Reuse the same slot
        let slot_pos = HEADER_SIZE + slot_id as usize * SLOT_SIZE;
        page[slot_pos..slot_pos + 2].copy_from_slice(&(free_offset as u16).to_le_bytes());
        page[slot_pos + 2..slot_pos + 4].copy_from_slice(&(row_len as u16).to_le_bytes());
        page[2..4].copy_from_slice(&(free_offset as u16).to_le_bytes());

        pm.write_page(page_id, &page)?;
        Ok(())
    }

    /// Get total rows across given pages
    pub fn count_rows(
        &self,
        pm: &mut PageManager,
        pages: &[PageId],
        columns: &[ColumnDef],
    ) -> Result<usize> {
        let mut count = 0;
        for &pid in pages {
            let rows = self.scan_page(pm, pid, columns)?;
            count += rows.len();
        }
        Ok(count)
    }
}

fn serialize_row(row: &Row, _columns: &[ColumnDef]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&(row.values.len() as u32).to_le_bytes());
    for val in &row.values {
        match val {
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
                buf.write_all(bytes).unwrap();
            }
            Value::Bool(b) => {
                buf.push(4);
                buf.push(if *b { 1 } else { 0 });
            }
        }
    }
    buf
}

fn deserialize_row(data: &[u8], _columns: &[ColumnDef]) -> Result<Row> {
    if data.len() < 4 {
        return Err(MiniDbError::Storage("row data too short".to_string()));
    }
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let mut pos = 4;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        if pos >= data.len() {
            return Err(MiniDbError::Storage("row data truncated".to_string()));
        }
        let tag = data[pos];
        pos += 1;
        match tag {
            0 => values.push(Value::Null),
            1 => {
                if pos + 8 > data.len() {
                    return Err(MiniDbError::Storage(
                        "row data truncated for Int".to_string(),
                    ));
                }
                let i = i64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
                values.push(Value::Int(i));
                pos += 8;
            }
            2 => {
                if pos + 8 > data.len() {
                    return Err(MiniDbError::Storage(
                        "row data truncated for Real".to_string(),
                    ));
                }
                let r = f64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
                values.push(Value::Real(r));
                pos += 8;
            }
            3 => {
                if pos + 4 > data.len() {
                    return Err(MiniDbError::Storage(
                        "row data truncated for Text len".to_string(),
                    ));
                }
                let slen = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + slen > data.len() {
                    return Err(MiniDbError::Storage(
                        "row data truncated for Text".to_string(),
                    ));
                }
                let s = String::from_utf8_lossy(&data[pos..pos + slen]).to_string();
                values.push(Value::Text(s));
                pos += slen;
            }
            4 => {
                if pos >= data.len() {
                    return Err(MiniDbError::Storage(
                        "row data truncated for Bool".to_string(),
                    ));
                }
                values.push(Value::Bool(data[pos] != 0));
                pos += 1;
            }
            _ => return Err(MiniDbError::Storage(format!("unknown value tag: {tag}"))),
        }
    }
    Ok(Row::new(values))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ColumnType;
    use tempfile::tempdir;

    fn int_col(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            col_type: ColumnType::Int,
            primary_key: false,
            not_null: false,
            unique: false,
            default: None,
        }
    }

    fn text_col(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            col_type: ColumnType::Text,
            primary_key: false,
            not_null: false,
            unique: false,
            default: None,
        }
    }

    #[test]
    fn test_heap_insert_scan() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::create(&path).unwrap();
        let heap = HeapFile::new("test");
        let cols = vec![int_col("id"), text_col("name")];

        let pid = heap.alloc_page(&mut pm).unwrap();

        heap.insert_row(
            &mut pm,
            pid,
            &Row::new(vec![Value::Int(1), Value::Text("alice".to_string())]),
            &cols,
        )
        .unwrap();
        heap.insert_row(
            &mut pm,
            pid,
            &Row::new(vec![Value::Int(2), Value::Text("bob".to_string())]),
            &cols,
        )
        .unwrap();

        let rows = heap.scan_page(&mut pm, pid, &cols).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1.values[0], Value::Int(1));
        assert_eq!(rows[0].1.values[1], Value::Text("alice".to_string()));
        assert_eq!(rows[1].1.values[0], Value::Int(2));
    }

    #[test]
    fn test_heap_delete() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::create(&path).unwrap();
        let heap = HeapFile::new("test");
        let cols = vec![int_col("id")];

        let pid = heap.alloc_page(&mut pm).unwrap();
        heap.insert_row(&mut pm, pid, &Row::new(vec![Value::Int(1)]), &cols)
            .unwrap();
        heap.insert_row(&mut pm, pid, &Row::new(vec![Value::Int(2)]), &cols)
            .unwrap();

        heap.delete_row(&mut pm, pid, 0).unwrap();

        let rows = heap.scan_page(&mut pm, pid, &cols).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.values[0], Value::Int(2));
    }

    #[test]
    fn test_serialize_deserialize() {
        let cols = vec![int_col("a"), text_col("b")];
        let row = Row::new(vec![Value::Int(42), Value::Text("hello".to_string())]);
        let data = serialize_row(&row, &cols);
        let deserialized = deserialize_row(&data, &cols).unwrap();
        assert_eq!(deserialized.values, row.values);
    }
}

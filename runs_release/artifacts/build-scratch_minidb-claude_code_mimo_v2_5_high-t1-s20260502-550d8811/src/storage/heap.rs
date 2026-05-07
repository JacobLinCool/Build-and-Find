use crate::error::{MiniDbError, Result};
use crate::storage::page::{PageId, PageManager, PAGE_SIZE};
use crate::types::{Row, RowId, Value};
use std::collections::HashMap;
use std::path::Path;

// Page layout for heap files:
// Bytes 0..2: number of rows in this page (u16)
// Bytes 2..4: free space offset (u16) - start of free area
// Bytes 4..: serialized rows, packed from the beginning
// Row format: [num_cols:u16][col0_len:u32][col0_data:bytes]...

pub struct HeapFile {
    pm: PageManager,
    table_pages: HashMap<String, Vec<PageId>>,
    _next_row_id: u64,
}

fn serialize_row(row: &Row) -> Vec<u8> {
    let mut buf = Vec::new();
    let num_cols = row.len() as u16;
    buf.extend_from_slice(&num_cols.to_le_bytes());
    for val in row {
        match val {
            Value::Null => {
                buf.extend_from_slice(&0u32.to_le_bytes()); // tag
                buf.extend_from_slice(&0u32.to_le_bytes()); // len=0
            }
            Value::Int(v) => {
                buf.extend_from_slice(&1u32.to_le_bytes());
                buf.extend_from_slice(&8u32.to_le_bytes());
                buf.extend_from_slice(&v.to_le_bytes());
            }
            Value::Real(v) => {
                buf.extend_from_slice(&2u32.to_le_bytes());
                buf.extend_from_slice(&8u32.to_le_bytes());
                buf.extend_from_slice(&v.0.to_le_bytes());
            }
            Value::Text(s) => {
                buf.extend_from_slice(&3u32.to_le_bytes());
                let bytes = s.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(bytes);
            }
            Value::Bool(v) => {
                buf.extend_from_slice(&4u32.to_le_bytes());
                buf.extend_from_slice(&1u32.to_le_bytes());
                buf.push(if *v { 1 } else { 0 });
            }
        }
    }
    buf
}

fn deserialize_row(data: &[u8]) -> Result<Row> {
    if data.len() < 2 {
        return Err(MiniDbError::Storage("row data too short".into()));
    }
    let num_cols = u16::from_le_bytes(data[0..2].try_into().unwrap()) as usize;
    let mut pos = 2;
    let mut row = Vec::with_capacity(num_cols);
    for _ in 0..num_cols {
        if pos + 8 > data.len() {
            return Err(MiniDbError::Storage("truncated row data".into()));
        }
        let tag = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
        let len = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        pos += 8;
        if pos + len > data.len() {
            return Err(MiniDbError::Storage("truncated row data".into()));
        }
        let val = match tag {
            0 => Value::Null,
            1 => {
                if len != 8 {
                    return Err(MiniDbError::Storage("invalid INT size".into()));
                }
                Value::Int(i64::from_le_bytes(data[pos..pos + 8].try_into().unwrap()))
            }
            2 => {
                if len != 8 {
                    return Err(MiniDbError::Storage("invalid REAL size".into()));
                }
                Value::Real(crate::types::RealOrd(f64::from_le_bytes(
                    data[pos..pos + 8].try_into().unwrap(),
                )))
            }
            3 => {
                let s = String::from_utf8(data[pos..pos + len].to_vec())
                    .map_err(|e| MiniDbError::Storage(format!("invalid UTF-8: {}", e)))?;
                Value::Text(s)
            }
            4 => {
                if len != 1 {
                    return Err(MiniDbError::Storage("invalid BOOL size".into()));
                }
                Value::Bool(data[pos] != 0)
            }
            _ => return Err(MiniDbError::Storage(format!("unknown value tag: {}", tag))),
        };
        pos += len;
        row.push(val);
    }
    Ok(row)
}

fn page_header_read(data: &[u8]) -> (u16, u16) {
    let count = u16::from_le_bytes(data[0..2].try_into().unwrap());
    let free_offset = u16::from_le_bytes(data[2..4].try_into().unwrap());
    (count, free_offset)
}

fn page_header_write(data: &mut [u8], count: u16, free_offset: u16) {
    data[0..2].copy_from_slice(&count.to_le_bytes());
    data[2..4].copy_from_slice(&free_offset.to_le_bytes());
}

impl HeapFile {
    pub fn open(path: &Path) -> Result<Self> {
        let mut pm = PageManager::open(path)?;
        let mut table_pages: HashMap<String, Vec<PageId>> = HashMap::new();
        let _next_row_id = 0u64;

        // Ensure page 0 exists as directory page
        if pm.num_pages() == 0 {
            pm.alloc_page()?; // page 0 = directory
        }

        // Read directory from page 0
        if pm.num_pages() > 0 {
            let dir_page = pm.read_page(PageId(0))?;
            let num_tables = u16::from_le_bytes(dir_page.data[0..2].try_into().unwrap());
            let mut pos = 2;
            for _ in 0..num_tables {
                if pos + 2 > PAGE_SIZE {
                    break;
                }
                let name_len =
                    u16::from_le_bytes(dir_page.data[pos..pos + 2].try_into().unwrap()) as usize;
                pos += 2;
                if pos + name_len > PAGE_SIZE {
                    break;
                }
                let name = String::from_utf8(dir_page.data[pos..pos + name_len].to_vec())
                    .unwrap_or_default();
                pos += name_len;
                let num_pages =
                    u16::from_le_bytes(dir_page.data[pos..pos + 2].try_into().unwrap()) as usize;
                pos += 2;
                let mut pages = Vec::new();
                for _ in 0..num_pages {
                    if pos + 8 > PAGE_SIZE {
                        break;
                    }
                    let pid = u64::from_le_bytes(dir_page.data[pos..pos + 8].try_into().unwrap());
                    pages.push(PageId(pid));
                    pos += 8;
                }
                if !name.is_empty() {
                    table_pages.insert(name, pages);
                }
            }
        }

        Ok(HeapFile {
            pm,
            table_pages,
            _next_row_id,
        })
    }

    pub fn open_with_tables(path: &Path, _table_names: &[String]) -> Result<Self> {
        let mut pm = PageManager::open(path)?;
        let mut table_pages: HashMap<String, Vec<PageId>> = HashMap::new();

        // We'll store table pages as a simple list per table
        // In a real system, there'd be a directory page
        // For now, we use page 0 as a directory page
        if pm.num_pages() > 0 {
            let dir_page = pm.read_page(PageId(0))?;
            let num_tables = u16::from_le_bytes(dir_page.data[0..2].try_into().unwrap());
            let mut pos = 2;
            for _ in 0..num_tables {
                if pos + 4 > PAGE_SIZE {
                    break;
                }
                let name_len =
                    u16::from_le_bytes(dir_page.data[pos..pos + 2].try_into().unwrap()) as usize;
                pos += 2;
                if pos + name_len > PAGE_SIZE {
                    break;
                }
                let name = String::from_utf8(dir_page.data[pos..pos + name_len].to_vec())
                    .unwrap_or_default();
                pos += name_len;
                let num_pages =
                    u16::from_le_bytes(dir_page.data[pos..pos + 2].try_into().unwrap()) as usize;
                pos += 2;
                let mut pages = Vec::new();
                for _ in 0..num_pages {
                    if pos + 8 > PAGE_SIZE {
                        break;
                    }
                    let pid = u64::from_le_bytes(dir_page.data[pos..pos + 8].try_into().unwrap());
                    pages.push(PageId(pid));
                    pos += 8;
                }
                table_pages.insert(name, pages);
            }
        }

        let _next_row_id = table_pages.values().flatten().count() as u64 * 1024;

        Ok(HeapFile {
            pm,
            table_pages,
            _next_row_id,
        })
    }

    fn save_directory(&mut self) -> Result<()> {
        // Ensure page 0 exists for the directory
        if self.pm.num_pages() == 0 {
            self.pm.alloc_page()?;
        }

        let mut dir_data = [0u8; PAGE_SIZE];
        let num_tables = self.table_pages.len() as u16;
        dir_data[0..2].copy_from_slice(&num_tables.to_le_bytes());
        let mut pos = 2;

        for (name, pages) in &self.table_pages {
            let name_bytes = name.as_bytes();
            if pos + 2 + name_bytes.len() + 2 + pages.len() * 8 > PAGE_SIZE {
                return Err(MiniDbError::Storage("directory page full".into()));
            }
            dir_data[pos..pos + 2].copy_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            pos += 2;
            dir_data[pos..pos + name_bytes.len()].copy_from_slice(name_bytes);
            pos += name_bytes.len();
            dir_data[pos..pos + 2].copy_from_slice(&(pages.len() as u16).to_le_bytes());
            pos += 2;
            for pid in pages {
                dir_data[pos..pos + 8].copy_from_slice(&pid.0.to_le_bytes());
                pos += 8;
            }
        }

        let mut dir_page = crate::storage::page::Page::new(PageId(0));
        dir_page.data = dir_data;
        self.pm.write_page(&dir_page)?;
        Ok(())
    }

    pub fn insert_row(&mut self, table: &str, row: &Row) -> Result<RowId> {
        let serialized = serialize_row(row);
        let row_len = serialized.len();

        // Find a page with enough free space, or allocate a new one
        let mut target_page_id = None;
        if let Some(pages) = self.table_pages.get(table) {
            for &pid in pages.iter() {
                let page = self.pm.read_page(pid)?;
                let (_count, free_offset) = page_header_read(&page.data);
                let used = free_offset as usize;
                let needed = 2 + row_len;
                if used + needed <= PAGE_SIZE {
                    target_page_id = Some(pid);
                    break;
                }
            }
        }

        let pid = if let Some(id) = target_page_id {
            id
        } else {
            let new_pid = self.pm.alloc_page()?;
            self.table_pages
                .entry(table.to_string())
                .or_default()
                .push(new_pid);
            self.save_directory()?;
            new_pid
        };

        let slot = if let Some(pages) = self.table_pages.get(table) {
            if let Some(_pos) = pages.iter().position(|p| *p == pid) {
                // Count existing rows on this page
                let page = self.pm.read_page(pid)?;
                let (count, _free_offset) = page_header_read(&page.data);
                count as u64
            } else {
                0
            }
        } else {
            0
        };
        let row_id_val = pid.0 * 1024 + slot;

        let mut page = self.pm.read_page(pid)?;
        let (count, free_offset) = page_header_read(&page.data);

        // If free_offset is 0, it's a fresh page - data starts after header (4 bytes)
        let mut new_offset = if free_offset == 0 {
            4
        } else {
            free_offset as usize
        };
        let row_len_u16 = row_len as u16;
        if new_offset + 2 + row_len > PAGE_SIZE {
            return Err(MiniDbError::Storage("page full".into()));
        }
        page.data[new_offset..new_offset + 2].copy_from_slice(&row_len_u16.to_le_bytes());
        new_offset += 2;
        page.data[new_offset..new_offset + row_len].copy_from_slice(&serialized);
        new_offset += row_len;

        page_header_write(&mut page.data, count + 1, new_offset as u16);
        self.pm.write_page(&page)?;

        Ok(RowId(row_id_val))
    }

    pub fn scan_table(&mut self, table: &str) -> Result<Vec<(RowId, Row)>> {
        let pages = match self.table_pages.get(table) {
            Some(p) => p.clone(),
            None => return Ok(Vec::new()),
        };

        let mut results = Vec::new();
        let _slot_counter: u64 = 0;

        for pid in &pages {
            let page = self.pm.read_page(*pid)?;
            let (count, free_offset) = page_header_read(&page.data);
            let mut offset = 4usize; // skip header
            let mut current_slot = 0u16;

            for _ in 0..count {
                if offset + 2 > free_offset as usize {
                    break;
                }
                let row_len =
                    u16::from_le_bytes(page.data[offset..offset + 2].try_into().unwrap()) as usize;
                offset += 2;

                if row_len == 0 {
                    // Deleted row, skip
                    current_slot += 1;
                    continue;
                }

                if offset + row_len > free_offset as usize {
                    break;
                }
                match deserialize_row(&page.data[offset..offset + row_len]) {
                    Ok(row) => {
                        let rid = RowId(pid.0 * 1024 + current_slot as u64);
                        results.push((rid, row));
                    }
                    Err(e) => {
                        log::warn!("Failed to deserialize row: {}", e);
                    }
                }
                offset += row_len;
                current_slot += 1;
            }
        }

        Ok(results)
    }

    pub fn update_row(&mut self, table: &str, row_id: &RowId, new_row: &Row) -> Result<()> {
        // Read all rows, modify the target, rewrite the page
        let pages = self.table_pages.get(table).cloned().unwrap_or_default();
        let target_page = row_id.page_id();
        let target_slot = row_id.slot();

        for pid in &pages {
            if pid.0 == target_page {
                // Read all live rows from this page
                let mut rows: Vec<(u16, Vec<u8>)> = Vec::new();
                {
                    let page = self.pm.read_page(*pid)?;
                    let (count, free_offset) = page_header_read(&page.data);
                    let mut offset = 4usize;
                    let mut current_slot: u16 = 0;

                    for _ in 0..count {
                        if offset + 2 > free_offset as usize {
                            break;
                        }
                        let row_len =
                            u16::from_le_bytes(page.data[offset..offset + 2].try_into().unwrap())
                                as usize;
                        offset += 2;
                        if row_len == 0 {
                            current_slot += 1;
                            continue;
                        }
                        if offset + row_len > free_offset as usize {
                            break;
                        }
                        let data = page.data[offset..offset + row_len].to_vec();
                        rows.push((current_slot, data));
                        offset += row_len;
                        current_slot += 1;
                    }
                }

                // Find and replace the target row
                let mut found = false;
                for (slot, data) in rows.iter_mut() {
                    if *slot == target_slot {
                        *data = serialize_row(new_row);
                        found = true;
                        break;
                    }
                }

                if !found {
                    return Err(MiniDbError::Runtime("row not found for update".into()));
                }

                // Rewrite the page
                let mut page = self.pm.read_page(*pid)?;
                let new_count = rows.len() as u16;
                let mut new_offset = 4usize;
                for (_, data) in &rows {
                    let row_len = data.len() as u16;
                    page.data[new_offset..new_offset + 2].copy_from_slice(&row_len.to_le_bytes());
                    new_offset += 2;
                    page.data[new_offset..new_offset + data.len()].copy_from_slice(data);
                    new_offset += data.len();
                }
                page_header_write(&mut page.data, new_count, new_offset as u16);
                self.pm.write_page(&page)?;
                return Ok(());
            }
        }
        Err(MiniDbError::Runtime("row not found for update".into()))
    }

    pub fn delete_row(&mut self, table: &str, row_id: &RowId) -> Result<()> {
        let pages = self.table_pages.get(table).cloned().unwrap_or_default();
        let target_page = row_id.page_id();
        let target_slot = row_id.slot();

        for pid in &pages {
            if pid.0 == target_page {
                // Read all live rows from this page
                let mut rows: Vec<(u16, Vec<u8>)> = Vec::new();
                {
                    let page = self.pm.read_page(*pid)?;
                    let (count, free_offset) = page_header_read(&page.data);
                    let mut offset = 4usize;
                    let mut current_slot: u16 = 0;

                    for _ in 0..count {
                        if offset + 2 > free_offset as usize {
                            break;
                        }
                        let row_len =
                            u16::from_le_bytes(page.data[offset..offset + 2].try_into().unwrap())
                                as usize;
                        offset += 2;
                        if row_len == 0 {
                            current_slot += 1;
                            continue;
                        }
                        if offset + row_len > free_offset as usize {
                            break;
                        }
                        let data = page.data[offset..offset + row_len].to_vec();
                        rows.push((current_slot, data));
                        offset += row_len;
                        current_slot += 1;
                    }
                }

                // Find and remove the target row
                let original_len = rows.len();
                rows.retain(|(slot, _)| *slot != target_slot);

                if rows.len() == original_len {
                    return Err(MiniDbError::Runtime("row not found for delete".into()));
                }

                // Rewrite the page
                let mut page = self.pm.read_page(*pid)?;
                let new_count = rows.len() as u16;
                let mut new_offset = 4usize;
                for (_, data) in &rows {
                    let row_len = data.len() as u16;
                    page.data[new_offset..new_offset + 2].copy_from_slice(&row_len.to_le_bytes());
                    new_offset += 2;
                    page.data[new_offset..new_offset + data.len()].copy_from_slice(data);
                    new_offset += data.len();
                }
                page_header_write(&mut page.data, new_count, new_offset as u16);
                self.pm.write_page(&page)?;
                return Ok(());
            }
        }
        Err(MiniDbError::Runtime("row not found for delete".into()))
    }

    pub fn page_manager_mut(&mut self) -> &mut PageManager {
        &mut self.pm
    }

    pub fn page_manager(&self) -> &PageManager {
        &self.pm
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_insert_and_scan() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("heap.db");
        let mut heap = HeapFile::open(&path).unwrap();

        let row1 = vec![Value::Int(1), Value::Text("Alice".into())];
        let row2 = vec![Value::Int(2), Value::Text("Bob".into())];

        heap.insert_row("users", &row1).unwrap();
        heap.insert_row("users", &row2).unwrap();

        let rows = heap.scan_table("users").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, row1);
        assert_eq!(rows[1].1, row2);
    }

    #[test]
    fn test_delete_row() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("heap.db");
        let mut heap = HeapFile::open(&path).unwrap();

        let row1 = vec![Value::Int(1), Value::Text("Alice".into())];
        let rid = heap.insert_row("users", &row1).unwrap();

        heap.delete_row("users", &rid).unwrap();
        let rows = heap.scan_table("users").unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn test_serialize_deserialize() {
        let row = vec![
            Value::Null,
            Value::Int(42),
            Value::Real(crate::types::RealOrd(3.14)),
            Value::Text("hello".into()),
            Value::Bool(true),
        ];
        let encoded = serialize_row(&row);
        let decoded = deserialize_row(&encoded).unwrap();
        assert_eq!(row, decoded);
    }
}

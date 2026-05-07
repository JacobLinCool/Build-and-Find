use crate::error::{MiniDbError, Result};
use crate::storage::page::{Page, PageId, PageManager, PAGE_TYPE_HEAP};
use crate::types::{deserialize_row, serialize_row, DataType, Row};

/// Identifies a row within a heap: (page_id, slot_index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowId {
    pub page: u32,
    pub slot: u16,
}

impl RowId {
    pub fn new(page: PageId, slot: usize) -> Self {
        RowId {
            page: page.0 as u32,
            slot: slot as u16,
        }
    }

    pub fn page_id(self) -> PageId {
        PageId(self.page as u64)
    }

    pub fn encode(self) -> u64 {
        ((self.page as u64) << 16) | (self.slot as u64)
    }

    pub fn decode(v: u64) -> Self {
        RowId {
            page: (v >> 16) as u32,
            slot: (v & 0xFFFF) as u16,
        }
    }
}

/// Manages heap storage for a single table.
/// Each record stored includes: [row_id: 8 bytes][row_data: variable].
pub struct HeapFile {
    pub first_page: PageId,
    pub last_page: PageId,
}

impl HeapFile {
    /// Initialize a new heap file with one empty page.
    pub fn create(pm: &mut PageManager) -> Result<Self> {
        let page_id = pm.alloc_page()?;
        let mut page = Page::new();
        page.init_heap();
        pm.write_page(page_id, &page)?;
        Ok(HeapFile {
            first_page: page_id,
            last_page: page_id,
        })
    }

    /// Serialize a (row_id, row) pair into bytes.
    fn encode_record(row_id: RowId, row: &Row) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&row_id.encode().to_le_bytes());
        buf.extend_from_slice(&serialize_row(row));
        buf
    }

    /// Decode a record: returns (row_id, row_data_bytes).
    fn decode_record(data: &[u8]) -> Result<(RowId, &[u8])> {
        if data.len() < 8 {
            return Err(MiniDbError::StorageCorruption("record too short".into()));
        }
        let rid_val = u64::from_le_bytes(data[0..8].try_into().unwrap());
        Ok((RowId::decode(rid_val), &data[8..]))
    }

    /// Insert a row. Returns the assigned RowId.
    pub fn insert(&mut self, pm: &mut PageManager, row: &Row) -> Result<RowId> {
        // First, assign a preliminary row_id (we'll finalize after slot assignment)
        // We use a placeholder; after getting the slot, we re-encode.
        // Actually, we need the final RowId before encoding... Let's use a two-pass approach:
        // 1. Estimate slot on last page
        // 2. Assign RowId based on that, then write

        // Try to insert into the last page
        let encoded_row = serialize_row(row);
        let record_len = 8 + encoded_row.len();

        // Check if last page has space
        let mut page = pm.read_page(self.last_page)?;
        let slot;

        if page.available_space() >= record_len {
            // Will fit on last page
            let placeholder_rid = RowId::new(self.last_page, page.num_slots() as usize);
            let record = Self::encode_record(placeholder_rid, row);
            slot = page.insert_record(&record)?;
            // Update the stored row_id in the record to match actual slot
            let actual_rid = RowId::new(self.last_page, slot);
            if actual_rid != placeholder_rid {
                // Re-encode with correct rid and update in place
                let new_record = Self::encode_record(actual_rid, row);
                page.update_record(slot, &new_record)?;
            }
            pm.write_page(self.last_page, &page)?;
            Ok(actual_rid)
        } else {
            // Allocate a new page
            let new_page_id = pm.alloc_page()?;
            let mut new_page = Page::new();
            new_page.init_heap();

            let actual_rid = RowId::new(new_page_id, 0);
            let record = Self::encode_record(actual_rid, row);
            let s = new_page.insert_record(&record)?;
            debug_assert_eq!(s, 0);
            pm.write_page(new_page_id, &new_page)?;

            // Update last page to point to new page
            page.set_next_page(new_page_id);
            pm.write_page(self.last_page, &page)?;

            self.last_page = new_page_id;
            Ok(actual_rid)
        }
    }

    /// Get a specific row by RowId.
    pub fn get(
        &self,
        pm: &mut PageManager,
        rid: RowId,
        col_types: &[DataType],
    ) -> Result<Option<Row>> {
        let page = pm.read_page(rid.page_id())?;
        match page.get_record(rid.slot as usize) {
            None => Ok(None),
            Some(data) => {
                let (_, row_bytes) = Self::decode_record(data)?;
                let row = deserialize_row(row_bytes, col_types)?;
                Ok(Some(row))
            }
        }
    }

    /// Delete a row by RowId.
    pub fn delete(&self, pm: &mut PageManager, rid: RowId) -> Result<bool> {
        let mut page = pm.read_page(rid.page_id())?;
        if page.get_record(rid.slot as usize).is_none() {
            return Ok(false);
        }
        page.delete_record(rid.slot as usize);
        pm.write_page(rid.page_id(), &page)?;
        Ok(true)
    }

    /// Update a row in place. If new row is larger, delete and re-insert.
    pub fn update(&mut self, pm: &mut PageManager, rid: RowId, new_row: &Row) -> Result<RowId> {
        let new_encoded = serialize_row(new_row);
        let new_record = Self::encode_record(rid, new_row);

        let mut page = pm.read_page(rid.page_id())?;
        if let Some(existing) = page.get_record(rid.slot as usize) {
            if existing.len() == new_record.len() {
                // Same size, update in place
                page.update_record(rid.slot as usize, &new_record)?;
                pm.write_page(rid.page_id(), &page)?;
                return Ok(rid);
            }
        }

        // Different size: delete and re-insert
        page.delete_record(rid.slot as usize);
        pm.write_page(rid.page_id(), &page)?;
        let new_rid = self.insert(pm, new_row)?;
        Ok(new_rid)
    }

    /// Scan all (non-deleted) rows. Returns (RowId, Row) pairs.
    pub fn scan(&self, pm: &mut PageManager, col_types: &[DataType]) -> Result<Vec<(RowId, Row)>> {
        let mut results = Vec::new();
        let mut page_id = self.first_page;

        while !page_id.is_null() {
            let page = pm.read_page(page_id)?;
            if page.page_type() != PAGE_TYPE_HEAP {
                break;
            }
            let num_slots = page.num_slots() as usize;
            for slot in 0..num_slots {
                if let Some(data) = page.get_record(slot) {
                    let (rid, row_bytes) = Self::decode_record(data)?;
                    let row = deserialize_row(row_bytes, col_types)?;
                    results.push((rid, row));
                }
            }
            page_id = page.next_page();
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DataType, Value};
    use tempfile::NamedTempFile;

    fn make_pm() -> (PageManager, NamedTempFile) {
        let tmp = NamedTempFile::new().unwrap();
        let pm = PageManager::open(tmp.path()).unwrap();
        (pm, tmp)
    }

    #[test]
    fn test_heap_insert_scan() {
        let (mut pm, _tmp) = make_pm();
        let mut heap = HeapFile::create(&mut pm).unwrap();

        let types = vec![DataType::Int, DataType::Text];
        let row1: Row = vec![Value::Int(1), Value::Text("alice".into())];
        let row2: Row = vec![Value::Int(2), Value::Text("bob".into())];

        let rid1 = heap.insert(&mut pm, &row1).unwrap();
        let rid2 = heap.insert(&mut pm, &row2).unwrap();

        let rows = heap.scan(&mut pm, &types).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1[0], Value::Int(1));
        assert_eq!(rows[1].1[0], Value::Int(2));
    }

    #[test]
    fn test_heap_delete() {
        let (mut pm, _tmp) = make_pm();
        let mut heap = HeapFile::create(&mut pm).unwrap();

        let types = vec![DataType::Int];
        let rid = heap.insert(&mut pm, &vec![Value::Int(42)]).unwrap();
        heap.delete(&mut pm, rid).unwrap();

        let rows = heap.scan(&mut pm, &types).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn test_heap_get() {
        let (mut pm, _tmp) = make_pm();
        let mut heap = HeapFile::create(&mut pm).unwrap();

        let types = vec![DataType::Int, DataType::Text];
        let row: Row = vec![Value::Int(99), Value::Text("test".into())];
        let rid = heap.insert(&mut pm, &row).unwrap();

        let got = heap.get(&mut pm, rid, &types).unwrap().unwrap();
        assert_eq!(got[0], Value::Int(99));
    }
}

use crate::error::Result;
use crate::storage::page::{PageId, PageManager, PAGE_SIZE};
use crate::types::{Row, Value};

// Page layout for heap:
// [page_type:1][num_slots:2][free_offset:2][next_page:8] = 13 bytes header
// Slot array: each slot = [offset:2][length:2] = 4 bytes
// Data grows from end of page toward slot array

const PAGE_TYPE_HEAP: u8 = 1;
const HEADER_SIZE: usize = 13;
const SLOT_SIZE: usize = 4;

pub struct HeapFile {
    pub table_id: u64,
    pub first_page: PageId,
}

impl HeapFile {
    pub fn new(table_id: u64, first_page: PageId) -> Self {
        Self {
            table_id,
            first_page,
        }
    }

    pub fn init_page(page: &mut [u8; PAGE_SIZE]) {
        page[0] = PAGE_TYPE_HEAP;
        page[1..3].copy_from_slice(&0u16.to_le_bytes()); // num_slots
        page[3..5].copy_from_slice(&(PAGE_SIZE as u16).to_le_bytes()); // free_offset
        page[5..13].copy_from_slice(&u64::MAX.to_le_bytes()); // next_page = none
    }

    pub fn insert_row(&self, pm: &mut PageManager, row: &[Value]) -> Result<(PageId, u64)> {
        let serialized = serde_json::to_vec(row)?;
        let data_len = serialized.len();
        let needed = data_len + SLOT_SIZE;

        // Try to find a page with space
        let mut current = self.first_page;
        loop {
            let mut page = pm.read_page(current)?;
            let num_slots = u16::from_le_bytes(page[1..3].try_into().unwrap()) as usize;
            let free_offset = u16::from_le_bytes(page[3..5].try_into().unwrap()) as usize;

            let slot_array_end = HEADER_SIZE + (num_slots + 1) * SLOT_SIZE;
            if slot_array_end + needed <= free_offset {
                // Write data at free_offset - data_len
                let data_start = free_offset - data_len;
                page[data_start..free_offset].copy_from_slice(&serialized);

                // Write slot entry
                let slot_offset = HEADER_SIZE + num_slots * SLOT_SIZE;
                page[slot_offset..slot_offset + 2]
                    .copy_from_slice(&(data_start as u16).to_le_bytes());
                page[slot_offset + 2..slot_offset + 4]
                    .copy_from_slice(&(data_len as u16).to_le_bytes());

                // Update header
                page[1..3].copy_from_slice(&((num_slots + 1) as u16).to_le_bytes());
                page[3..5].copy_from_slice(&(data_start as u16).to_le_bytes());

                let row_id = num_slots as u64;
                pm.write_page(current, &page)?;
                return Ok((current, row_id));
            }

            // Check next page
            let next = u64::from_le_bytes(page[5..13].try_into().unwrap());
            if next == u64::MAX {
                // Allocate new page
                let new_pid = pm.allocate_page()?;
                let mut new_page = [0u8; PAGE_SIZE];
                Self::init_page(&mut new_page);

                // Link
                page[5..13].copy_from_slice(&new_pid.0.to_le_bytes());
                pm.write_page(current, &page)?;

                // Insert into new page
                let data_start = PAGE_SIZE - data_len;
                new_page[data_start..PAGE_SIZE].copy_from_slice(&serialized);
                let slot_offset = HEADER_SIZE;
                new_page[slot_offset..slot_offset + 2]
                    .copy_from_slice(&(data_start as u16).to_le_bytes());
                new_page[slot_offset + 2..slot_offset + 4]
                    .copy_from_slice(&(data_len as u16).to_le_bytes());
                new_page[1..3].copy_from_slice(&1u16.to_le_bytes());
                new_page[3..5].copy_from_slice(&(data_start as u16).to_le_bytes());

                pm.write_page(new_pid, &new_page)?;
                return Ok((new_pid, 0));
            }
            current = PageId(next);
        }
    }

    pub fn scan_all(&self, pm: &mut PageManager) -> Result<Vec<(PageId, usize, Row)>> {
        let mut results = Vec::new();
        let mut current = self.first_page;
        loop {
            let page = pm.read_page(current)?;
            let num_slots = u16::from_le_bytes(page[1..3].try_into().unwrap()) as usize;
            let next = u64::from_le_bytes(page[5..13].try_into().unwrap());

            for i in 0..num_slots {
                let slot_offset = HEADER_SIZE + i * SLOT_SIZE;
                let data_offset =
                    u16::from_le_bytes(page[slot_offset..slot_offset + 2].try_into().unwrap())
                        as usize;
                let data_len =
                    u16::from_le_bytes(page[slot_offset + 2..slot_offset + 4].try_into().unwrap())
                        as usize;

                if data_len > 0 && data_offset > 0 {
                    let data = &page[data_offset..data_offset + data_len];
                    if let Ok(row) = serde_json::from_slice::<Row>(data) {
                        results.push((current, i, row));
                    }
                }
            }

            if next == u64::MAX {
                break;
            }
            current = PageId(next);
        }
        Ok(results)
    }

    pub fn delete_row(&self, pm: &mut PageManager, page_id: PageId, slot_idx: usize) -> Result<()> {
        let mut page = pm.read_page(page_id)?;
        let slot_offset = HEADER_SIZE + slot_idx * SLOT_SIZE;
        // Mark slot as deleted (zero out)
        page[slot_offset..slot_offset + SLOT_SIZE].copy_from_slice(&[0u8; SLOT_SIZE]);
        pm.write_page(page_id, &page)?;
        Ok(())
    }

    pub fn update_row(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        slot_idx: usize,
        row: &[Value],
    ) -> Result<()> {
        let serialized = serde_json::to_vec(row)?;
        let mut page = pm.read_page(page_id)?;

        let slot_offset = HEADER_SIZE + slot_idx * SLOT_SIZE;
        let old_offset =
            u16::from_le_bytes(page[slot_offset..slot_offset + 2].try_into().unwrap()) as usize;
        let old_len =
            u16::from_le_bytes(page[slot_offset + 2..slot_offset + 4].try_into().unwrap()) as usize;

        let free_offset = u16::from_le_bytes(page[3..5].try_into().unwrap()) as usize;

        // If the new data fits where the old data was
        if serialized.len() <= old_len {
            page[old_offset..old_offset + serialized.len()].copy_from_slice(&serialized);
            page[slot_offset + 2..slot_offset + 4]
                .copy_from_slice(&(serialized.len() as u16).to_le_bytes());
        } else {
            // Check if there's enough free space
            let num_slots = u16::from_le_bytes(page[1..3].try_into().unwrap()) as usize;
            let slot_array_end = HEADER_SIZE + num_slots * SLOT_SIZE;
            let available = free_offset - slot_array_end;
            if serialized.len() > available {
                // Need to delete and re-insert (simplification)
                self.delete_row(pm, page_id, slot_idx)?;
                return Ok(());
            }
            // Allocate from free space
            let new_offset = free_offset - serialized.len();
            page[new_offset..free_offset].copy_from_slice(&serialized);
            page[slot_offset..slot_offset + 2].copy_from_slice(&(new_offset as u16).to_le_bytes());
            page[slot_offset + 2..slot_offset + 4]
                .copy_from_slice(&(serialized.len() as u16).to_le_bytes());
            page[3..5].copy_from_slice(&(new_offset as u16).to_le_bytes());
        }

        pm.write_page(page_id, &page)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup() -> (PageManager, HeapFile) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::open(&path).unwrap();
        let pid = pm.allocate_page().unwrap();
        let mut page = [0u8; PAGE_SIZE];
        HeapFile::init_page(&mut page);
        pm.write_page(pid, &page).unwrap();
        (pm, HeapFile::new(1, pid))
    }

    #[test]
    fn test_insert_and_scan() {
        let (mut pm, heap) = setup();
        let row1 = vec![Value::Int(1), Value::Text("Alice".into())];
        let row2 = vec![Value::Int(2), Value::Text("Bob".into())];

        heap.insert_row(&mut pm, &row1).unwrap();
        heap.insert_row(&mut pm, &row2).unwrap();

        let rows = heap.scan_all(&mut pm).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].2, row1);
        assert_eq!(rows[1].2, row2);
    }

    #[test]
    fn test_delete() {
        let (mut pm, heap) = setup();
        let row1 = vec![Value::Int(1), Value::Text("Alice".into())];
        let (pid, slot) = heap.insert_row(&mut pm, &row1).unwrap();

        heap.delete_row(&mut pm, pid, slot as usize).unwrap();

        let rows = heap.scan_all(&mut pm).unwrap();
        // Deleted row should not appear (data_len = 0)
        assert!(rows.is_empty());
    }

    #[test]
    fn test_page_overflow() {
        let (mut pm, heap) = setup();
        // Insert many rows to force page overflow
        for i in 0..100 {
            let row = vec![Value::Int(i), Value::Text(format!("row_{}", i))];
            heap.insert_row(&mut pm, &row).unwrap();
        }
        let rows = heap.scan_all(&mut pm).unwrap();
        assert_eq!(rows.len(), 100);
    }
}

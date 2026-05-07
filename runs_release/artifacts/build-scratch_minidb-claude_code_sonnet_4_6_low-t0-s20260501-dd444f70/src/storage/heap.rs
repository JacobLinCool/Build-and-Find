use crate::error::{MiniDbError, Result};
use crate::storage::page::{HeapPage, Page, PageId, PageType, PAGE_SIZE};
use crate::storage::page_manager::PageManager;
use crate::types::{deserialize_row, serialize_row, Row, RowId};
use log::debug;

/// RowId encoding: upper 32 bits = page_id, lower 32 bits = slot_idx
pub fn encode_row_id(page_id: u64, slot_idx: u16) -> RowId {
    (page_id << 16) | (slot_idx as u64)
}

pub fn decode_row_id(row_id: RowId) -> (u64, u16) {
    let page_id = row_id >> 16;
    let slot_idx = (row_id & 0xFFFF) as u16;
    (page_id, slot_idx)
}

pub struct HeapFile {
    pub root_page: u64,
}

impl HeapFile {
    pub fn new(root_page: u64) -> Self {
        HeapFile { root_page }
    }

    pub fn init_root(&self, pm: &mut PageManager) -> Result<()> {
        let pid = PageId(self.root_page);
        // Ensure enough pages allocated
        while pm.num_pages <= self.root_page {
            pm.alloc_page()?;
        }
        let page = pm.read_page(pid)?;
        if page.page_type() != Some(PageType::Heap) {
            let mut hp = HeapPage::new(page);
            hp.init();
        }
        Ok(())
    }

    pub fn insert_row(&self, pm: &mut PageManager, row: &Row) -> Result<RowId> {
        let data = serialize_row(row);
        if data.len() > PAGE_SIZE / 2 {
            return Err(MiniDbError::Storage("row too large for page".to_string()));
        }

        let row_id = self.insert_into_chain(pm, &data)?;
        debug!("heap insert row_id={}", row_id);
        Ok(row_id)
    }

    fn insert_into_chain(&self, pm: &mut PageManager, data: &[u8]) -> Result<RowId> {
        let mut current_pid = self.root_page;
        loop {
            // Ensure page exists
            while pm.num_pages <= current_pid {
                pm.alloc_page()?;
            }
            let pid = PageId(current_pid);
            let page = pm.read_page(pid)?;

            if page.page_type() != Some(PageType::Heap) {
                let mut hp = HeapPage::new(page);
                hp.init();
            }

            let next_page = {
                let page = pm.read_page(PageId(current_pid))?;
                let hp = HeapPage { page };
                hp.next_page()
            };

            {
                let page = pm.read_page(PageId(current_pid))?;
                let mut hp = HeapPage { page };
                if hp.can_fit(data.len()) {
                    let slot = hp.insert_row(data).ok_or_else(|| {
                        MiniDbError::Storage("failed to insert row into heap page".to_string())
                    })?;
                    return Ok(encode_row_id(current_pid, slot));
                }
            }

            if next_page == 0 {
                // Allocate new page
                let new_pid = pm.alloc_page()?;
                {
                    let page = pm.read_page(PageId(current_pid))?;
                    let mut hp = HeapPage { page };
                    hp.set_next_page(new_pid.0);
                }
                {
                    let page = pm.read_page(new_pid)?;
                    let mut hp = HeapPage { page };
                    hp.init();
                }
                current_pid = new_pid.0;
            } else {
                current_pid = next_page;
            }
        }
    }

    pub fn get_row(&self, pm: &mut PageManager, row_id: RowId) -> Result<Option<Row>> {
        let (page_id, slot_idx) = decode_row_id(row_id);
        if page_id >= pm.num_pages {
            return Ok(None);
        }
        let page = pm.read_page(PageId(page_id))?;
        let hp = HeapPage { page };
        match hp.get_row_data(slot_idx) {
            None => Ok(None),
            Some(data) => {
                let row = deserialize_row(data)?;
                Ok(Some(row))
            }
        }
    }

    pub fn delete_row(&self, pm: &mut PageManager, row_id: RowId) -> Result<()> {
        let (page_id, slot_idx) = decode_row_id(row_id);
        if page_id >= pm.num_pages {
            return Err(MiniDbError::Runtime(format!("invalid row_id {}", row_id)));
        }
        let page = pm.read_page(PageId(page_id))?;
        let mut hp = HeapPage { page };
        hp.delete_row(slot_idx);
        Ok(())
    }

    pub fn update_row(&self, pm: &mut PageManager, row_id: RowId, row: &Row) -> Result<bool> {
        let data = serialize_row(row);
        let (page_id, slot_idx) = decode_row_id(row_id);
        if page_id >= pm.num_pages {
            return Err(MiniDbError::Runtime(format!("invalid row_id {}", row_id)));
        }
        let page = pm.read_page(PageId(page_id))?;
        let mut hp = HeapPage { page };
        Ok(hp.update_row(slot_idx, &data))
    }

    /// Scan all rows, calling f(row_id, row) for each non-deleted row.
    /// f returns Ok(true) to continue, Ok(false) to stop.
    pub fn scan<F>(&self, pm: &mut PageManager, mut f: F) -> Result<()>
    where
        F: FnMut(RowId, Row) -> Result<bool>,
    {
        let mut current_pid = self.root_page;
        loop {
            if current_pid >= pm.num_pages {
                break;
            }

            let num_slots;
            let next_page;
            {
                let page = pm.read_page(PageId(current_pid))?;
                if page.page_type() != Some(PageType::Heap) {
                    break;
                }
                let hp = HeapPage { page };
                num_slots = hp.num_slots();
                next_page = hp.next_page();
            }

            for slot_idx in 0..num_slots {
                let row_data = {
                    let page = pm.read_page(PageId(current_pid))?;
                    let hp = HeapPage { page };
                    hp.get_row_data(slot_idx).map(|d| d.to_vec())
                };

                if let Some(data) = row_data {
                    let row = deserialize_row(&data)?;
                    let row_id = encode_row_id(current_pid, slot_idx);
                    if !f(row_id, row)? {
                        return Ok(());
                    }
                }
            }

            if next_page == 0 {
                break;
            }
            current_pid = next_page;
        }
        Ok(())
    }

    /// Get all (row_id, row) pairs as a vec
    pub fn scan_all(&self, pm: &mut PageManager) -> Result<Vec<(RowId, Row)>> {
        let mut results = Vec::new();
        self.scan(pm, |row_id, row| {
            results.push((row_id, row));
            Ok(true)
        })?;
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;
    use tempfile::NamedTempFile;

    fn make_pm() -> (PageManager, NamedTempFile) {
        let tmp = NamedTempFile::new().unwrap();
        let pm = PageManager::open(tmp.path()).unwrap();
        (pm, tmp)
    }

    #[test]
    fn test_heap_insert_and_scan() {
        let (mut pm, _tmp) = make_pm();
        pm.alloc_page().unwrap(); // page 0 for root

        let heap = HeapFile::new(0);
        heap.init_root(&mut pm).unwrap();

        let row1 = vec![Value::Int(1), Value::Text("alice".to_string())];
        let row2 = vec![Value::Int(2), Value::Text("bob".to_string())];

        let rid1 = heap.insert_row(&mut pm, &row1).unwrap();
        let rid2 = heap.insert_row(&mut pm, &row2).unwrap();

        let rows = heap.scan_all(&mut pm).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, rid1);
        assert_eq!(rows[1].0, rid2);
    }

    #[test]
    fn test_heap_delete() {
        let (mut pm, _tmp) = make_pm();
        pm.alloc_page().unwrap();

        let heap = HeapFile::new(0);
        heap.init_root(&mut pm).unwrap();

        let row = vec![Value::Int(1)];
        let rid = heap.insert_row(&mut pm, &row).unwrap();
        heap.delete_row(&mut pm, rid).unwrap();

        let rows = heap.scan_all(&mut pm).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn test_heap_get_row() {
        let (mut pm, _tmp) = make_pm();
        pm.alloc_page().unwrap();

        let heap = HeapFile::new(0);
        heap.init_root(&mut pm).unwrap();

        let row = vec![Value::Int(42), Value::Bool(true)];
        let rid = heap.insert_row(&mut pm, &row).unwrap();

        let fetched = heap.get_row(&mut pm, rid).unwrap().unwrap();
        assert_eq!(fetched, row);
    }
}

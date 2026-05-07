use crate::error::{MiniDbError, Result};
use crate::storage::btree::{serialize_value_for_index, BTreeIndex};
use crate::storage::heap::HeapFile;
use crate::storage::page::{Page, PageId};
use crate::storage::wal::{replay_wal, WalRecord, WalWriter};
use crate::types::Value;
use log::debug;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct StorageManager {
    heap: HeapFile,
    wal: WalWriter,
    wal_path: PathBuf,
    btrees: HashMap<u64, BTreeIndex>,
    txn_id: u64,
}

impl StorageManager {
    pub fn open(db_path: &Path) -> Result<Self> {
        let wal_path = db_path.with_extension("db.wal");

        let mut heap = HeapFile::open(db_path)?;

        // Replay WAL for crash recovery
        let records = replay_wal(&wal_path)?;
        if !records.is_empty() {
            debug!("Replaying {} WAL records", records.len());
            Self::apply_wal_records(&mut heap, &records)?;
        }

        let wal = WalWriter::new(&wal_path)?;

        Ok(StorageManager {
            heap,
            wal,
            wal_path,
            btrees: HashMap::new(),
            txn_id: 0,
        })
    }

    fn apply_wal_records(heap: &mut HeapFile, records: &[WalRecord]) -> Result<()> {
        for record in records {
            match record {
                WalRecord::Write {
                    page_id,
                    offset,
                    data,
                    ..
                } => {
                    let mut page = heap.load_page(*page_id)?;
                    let offset = *offset as usize;
                    page.data[offset..offset + data.len()].copy_from_slice(data);
                    heap.write_page(&page)?;
                }
                WalRecord::PageCreate { page_id, .. } => {
                    let page = Page::new(PageId(*page_id));
                    heap.write_page(&page)?;
                }
                _ => {}
            }
        }
        heap.flush()?;
        Ok(())
    }

    pub fn begin_txn(&mut self) -> Result<u64> {
        self.txn_id += 1;
        self.wal.append(&WalRecord::BeginTxn {
            txn_id: self.txn_id,
        })?;
        debug!("Begin transaction {}", self.txn_id);
        Ok(self.txn_id)
    }

    pub fn commit_txn(&mut self) -> Result<()> {
        self.wal.append(&WalRecord::CommitTxn {
            txn_id: self.txn_id,
        })?;
        self.heap.flush()?;
        debug!("Commit transaction {}", self.txn_id);
        Ok(())
    }

    pub fn rollback_txn(&mut self) -> Result<()> {
        // Discard uncommitted changes - in this simple model,
        // we just don't flush. For full rollback we'd need to
        // keep before-images.
        debug!("Rollback transaction {}", self.txn_id);
        Ok(())
    }

    pub fn write_page_data(&mut self, page_id: u64, offset: u16, data: &[u8]) -> Result<()> {
        self.wal.append(&WalRecord::Write {
            txn_id: self.txn_id,
            page_id,
            offset,
            data: data.to_vec(),
        })?;

        let mut page = self.heap.load_page(page_id)?;
        let offset_usize = offset as usize;
        page.data[offset_usize..offset_usize + data.len()].copy_from_slice(data);
        self.heap.write_page(&page)?;
        Ok(())
    }

    pub fn create_page(&mut self, page_id: u64) -> Result<()> {
        self.wal.append(&WalRecord::PageCreate {
            txn_id: self.txn_id,
            page_id,
        })?;
        let page = Page::new(PageId(page_id));
        self.heap.write_page(&page)?;
        Ok(())
    }

    pub fn load_page(&mut self, page_id: u64) -> Result<Page> {
        self.heap.load_page(page_id)
    }

    pub fn write_page(&mut self, page: &Page) -> Result<()> {
        self.heap.write_page(page)
    }

    pub fn insert_row(&mut self, page_id: u64, row_data: &[u8]) -> Result<u16> {
        self.heap.insert_row(page_id, row_data)
    }

    pub fn get_row(&mut self, page_id: u64, slot: u16) -> Result<Vec<u8>> {
        self.heap.get_row(page_id, slot)
    }

    pub fn delete_row(&mut self, page_id: u64, slot: u16) -> Result<()> {
        self.heap.delete_row(page_id, slot)
    }

    pub fn scan_page(&mut self, page_id: u64) -> Result<Vec<(u16, Vec<u8>)>> {
        self.heap.scan_page(page_id)
    }

    pub fn num_rows(&mut self, page_id: u64) -> Result<usize> {
        self.heap.num_rows(page_id)
    }

    pub fn load_btree(&mut self, root_page: u64) -> Result<()> {
        if !self.btrees.contains_key(&root_page) {
            let mut btree = BTreeIndex::new(root_page);
            btree.init_root(&mut &mut self.heap)?;
            self.btrees.insert(root_page, btree);
        }
        Ok(())
    }

    pub fn btree_insert(
        &mut self,
        root_page: u64,
        value: &Value,
        page_id: u64,
        slot: u16,
    ) -> Result<()> {
        let key = serialize_value_for_index(value);
        let btree = self
            .btrees
            .get_mut(&root_page)
            .ok_or_else(|| MiniDbError::IndexError("btree not loaded".to_string()))?;
        btree.insert(key, page_id, slot, &mut self.heap)?;
        Ok(())
    }

    pub fn btree_search(&mut self, root_page: u64, value: &Value) -> Result<Vec<(u64, u16)>> {
        let key = serialize_value_for_index(value);
        let btree = self
            .btrees
            .get_mut(&root_page)
            .ok_or_else(|| MiniDbError::IndexError("btree not loaded".to_string()))?;
        btree.search(&key, &mut self.heap)
    }

    pub fn flush(&mut self) -> Result<()> {
        self.heap.flush()
    }

    pub fn truncate_wal(&mut self) -> Result<()> {
        self.wal.truncate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_storage_manager_basic() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");

        let mut mgr = StorageManager::open(&db_path).unwrap();

        // Create a page
        mgr.create_page(0).unwrap();

        // Insert a row
        let row = b"test row data";
        let slot = mgr.insert_row(0, row).unwrap();
        assert_eq!(slot, 0);

        // Read it back
        let data = mgr.get_row(0, 0).unwrap();
        assert_eq!(data, row);
    }

    #[test]
    fn test_transaction() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");

        let mut mgr = StorageManager::open(&db_path).unwrap();
        mgr.create_page(0).unwrap();

        mgr.begin_txn().unwrap();
        mgr.insert_row(0, b"row1").unwrap();
        mgr.commit_txn().unwrap();

        let data = mgr.get_row(0, 0).unwrap();
        assert_eq!(data, b"row1");
    }
}

use crate::error::Result;
use crate::storage::wal::{Wal, WalRecord};
use crate::storage::{PageId, Pager, PAGE_SIZE};
use crate::types::Value;
use std::sync::{Arc, Mutex};

const LEAF_MAGIC: u16 = 0x424C; // "BL"
const INTERNAL_MAGIC: u16 = 0x4249; // "BI"

pub struct BTreeIndex {
    pub index_id: u64,
    pub root_page: PageId,
    pager: Arc<Mutex<Pager>>,
}

impl BTreeIndex {
    pub fn new(index_id: u64, root_page: PageId, pager: Arc<Mutex<Pager>>) -> Self {
        Self {
            index_id,
            root_page,
            pager,
        }
    }

    pub fn create(index_id: u64, pager: &mut Arc<Mutex<Pager>>) -> Result<(Self, PageId)> {
        let pid = pager.lock().unwrap().alloc_page();
        let page = LeafPage::new(pid);
        pager.lock().unwrap().write_page(pid, &page.raw)?;
        Ok((Self::new(index_id, pid, pager.clone()), pid))
    }

    pub fn insert(
        &self,
        key: &Value,
        rid: u64,
        wal: &mut Wal,
        table_id: u64,
        txn_id: u64,
    ) -> Result<()> {
        let key_bytes = key.to_bytes();
        let root_raw = self.pager.lock().unwrap().read_page(self.root_page)?;
        let magic = u16::from_le_bytes(root_raw[0..2].try_into().unwrap());
        if magic == LEAF_MAGIC {
            let mut leaf = LeafPage::from_raw(self.root_page, root_raw);
            if leaf.can_insert_raw(&key_bytes) {
                leaf.insert_raw(&key_bytes, rid);
                wal.append(&WalRecord::PageWrite {
                    table_id,
                    page_id: self.root_page.0,
                    data: leaf.raw,
                })?;
                self.pager
                    .lock()
                    .unwrap()
                    .write_page(self.root_page, &leaf.raw)?;
                Ok(())
            } else {
                self.split_leaf_and_insert(self.root_page, &key_bytes, rid, wal, table_id, txn_id)
            }
        } else {
            self.insert_internal(self.root_page, &key_bytes, rid, wal, table_id, txn_id)
        }
    }

    fn insert_internal(
        &self,
        pid: PageId,
        key_bytes: &[u8],
        rid: u64,
        wal: &mut Wal,
        table_id: u64,
        txn_id: u64,
    ) -> Result<()> {
        let raw = self.pager.lock().unwrap().read_page(pid)?;
        let magic = u16::from_le_bytes(raw[0..2].try_into().unwrap());
        if magic == LEAF_MAGIC {
            let mut leaf = LeafPage::from_raw(pid, raw);
            if leaf.can_insert_raw(key_bytes) {
                leaf.insert_raw(key_bytes, rid);
                wal.append(&WalRecord::PageWrite {
                    table_id,
                    page_id: pid.0,
                    data: leaf.raw,
                })?;
                self.pager.lock().unwrap().write_page(pid, &leaf.raw)?;
                Ok(())
            } else {
                self.split_leaf_and_insert(pid, key_bytes, rid, wal, table_id, txn_id)
            }
        } else {
            let internal = InternalPage::from_raw(pid, raw);
            let child = internal.find_child_bytes(key_bytes);
            self.insert_internal(child, key_bytes, rid, wal, table_id, txn_id)
        }
    }

    fn split_leaf_and_insert(
        &self,
        pid: PageId,
        key_bytes: &[u8],
        rid: u64,
        wal: &mut Wal,
        table_id: u64,
        _txn_id: u64,
    ) -> Result<()> {
        let raw = self.pager.lock().unwrap().read_page(pid)?;
        let mut leaf = LeafPage::from_raw(pid, raw);
        let (left_entries, right_entries, sep_bytes) = leaf.split_raw();

        let new_pid = self.pager.lock().unwrap().alloc_page();

        let mut left = LeafPage::new(pid);
        left.next_leaf = new_pid.0;
        for (k, r) in &left_entries {
            left.insert_raw(k, *r);
        }

        let mut right = LeafPage::new(new_pid);
        for (k, r) in &right_entries {
            right.insert_raw(k, *r);
        }

        if key_bytes <= sep_bytes.as_slice() {
            left.insert_raw(key_bytes, rid);
        } else {
            right.insert_raw(key_bytes, rid);
        }

        wal.append(&WalRecord::PageWrite {
            table_id,
            page_id: pid.0,
            data: left.raw,
        })?;
        wal.append(&WalRecord::PageWrite {
            table_id,
            page_id: new_pid.0,
            data: right.raw,
        })?;
        self.pager.lock().unwrap().write_page(pid, &left.raw)?;
        self.pager.lock().unwrap().write_page(new_pid, &right.raw)?;

        // Create new root if needed
        if pid == self.root_page {
            let new_root_pid = self.pager.lock().unwrap().alloc_page();
            let mut new_root = InternalPage::new(new_root_pid);
            new_root.rightmost_child = new_pid.0;
            new_root.insert_key_raw(&sep_bytes, pid.0);
            wal.append(&WalRecord::PageWrite {
                table_id,
                page_id: new_root_pid.0,
                data: new_root.raw,
            })?;
            self.pager
                .lock()
                .unwrap()
                .write_page(new_root_pid, &new_root.raw)?;
        }

        Ok(())
    }

    pub fn search(&self, key: &Value) -> Result<Vec<u64>> {
        let key_bytes = key.to_bytes();
        self.search_page_bytes(self.root_page, &key_bytes)
    }

    fn search_page_bytes(&self, pid: PageId, key_bytes: &[u8]) -> Result<Vec<u64>> {
        let raw = self.pager.lock().unwrap().read_page(pid)?;
        let magic = u16::from_le_bytes(raw[0..2].try_into().unwrap());
        if magic == LEAF_MAGIC {
            let leaf = LeafPage::from_raw(pid, raw);
            Ok(leaf.find_raw(key_bytes))
        } else {
            let internal = InternalPage::from_raw(pid, raw);
            let child = internal.find_child_bytes(key_bytes);
            self.search_page_bytes(child, key_bytes)
        }
    }

    pub fn search_range(&self, low: Option<&Value>, high: Option<&Value>) -> Result<Vec<u64>> {
        let low_bytes = low.map(|v| v.to_bytes());
        let high_bytes = high.map(|v| v.to_bytes());
        let mut results = Vec::new();
        self.search_range_page(
            self.root_page,
            low_bytes.as_deref(),
            high_bytes.as_deref(),
            &mut results,
        )?;
        Ok(results)
    }

    fn search_range_page(
        &self,
        pid: PageId,
        low: Option<&[u8]>,
        high: Option<&[u8]>,
        results: &mut Vec<u64>,
    ) -> Result<()> {
        let raw = self.pager.lock().unwrap().read_page(pid)?;
        let magic = u16::from_le_bytes(raw[0..2].try_into().unwrap());
        if magic == LEAF_MAGIC {
            let leaf = LeafPage::from_raw(pid, raw);
            for (key, rid) in leaf.entries() {
                let below_high = high.map_or(true, |h| key[..] <= h[..]);
                let above_low = low.map_or(true, |l| key[..] >= l[..]);
                if below_high && above_low {
                    results.push(rid);
                }
            }
            if leaf.next_leaf != 0 {
                let need_more = high.map_or(true, |h| {
                    leaf.entries().last().map_or(false, |(k, _)| k[..] <= h[..])
                });
                if need_more {
                    self.search_range_page(PageId(leaf.next_leaf), low, high, results)?;
                }
            }
            Ok(())
        } else {
            let internal = InternalPage::from_raw(pid, raw);
            for child_pid in internal.all_children() {
                self.search_range_page(PageId(child_pid), low, high, results)?;
            }
            Ok(())
        }
    }

    pub fn apply_wal_replay(&self, records: &[WalRecord]) -> Result<()> {
        for rec in records {
            if let WalRecord::PageWrite { page_id, data, .. } = rec {
                self.pager
                    .lock()
                    .unwrap()
                    .write_page(PageId(*page_id), data)?;
            }
        }
        Ok(())
    }
}

struct LeafPage {
    page_id: PageId,
    raw: [u8; PAGE_SIZE],
    next_leaf: u64,
}

impl LeafPage {
    fn new(page_id: PageId) -> Self {
        let mut raw = [0u8; PAGE_SIZE];
        raw[0..2].copy_from_slice(&LEAF_MAGIC.to_le_bytes());
        Self {
            page_id,
            raw,
            next_leaf: 0,
        }
    }

    fn from_raw(page_id: PageId, raw: [u8; PAGE_SIZE]) -> Self {
        let next_leaf = u64::from_le_bytes(raw[4..12].try_into().unwrap());
        Self {
            page_id,
            raw,
            next_leaf,
        }
    }

    fn num_entries(&self) -> usize {
        u16::from_le_bytes(self.raw[2..4].try_into().unwrap()) as usize
    }

    fn set_num_entries(&mut self, n: usize) {
        self.raw[2..4].copy_from_slice(&(n as u16).to_le_bytes());
    }

    fn data_offset(&self) -> usize {
        12
    }

    fn entries_offset_end(&self) -> usize {
        let mut pos = self.data_offset();
        for _ in 0..self.num_entries() {
            if pos + 2 > PAGE_SIZE {
                return pos;
            }
            let klen = u16::from_le_bytes(self.raw[pos..pos + 2].try_into().unwrap()) as usize;
            pos += 2 + klen + 8;
        }
        pos
    }

    fn can_insert_raw(&self, key_bytes: &[u8]) -> bool {
        let needed = 2 + key_bytes.len() + 8;
        self.entries_offset_end() + needed <= PAGE_SIZE
    }

    fn insert_raw(&mut self, key_bytes: &[u8], rid: u64) {
        let entries: Vec<(Vec<u8>, u64)> = self.entries();
        let pos_in_list = entries.partition_point(|(k, _)| k[..] < key_bytes[..]);

        let mut all: Vec<(Vec<u8>, u64)> = entries;
        all.insert(pos_in_list, (key_bytes.to_vec(), rid));

        let mut pos = self.data_offset();
        self.set_num_entries(all.len());
        for (kb, r) in &all {
            self.raw[pos..pos + 2].copy_from_slice(&(kb.len() as u16).to_le_bytes());
            pos += 2;
            self.raw[pos..pos + kb.len()].copy_from_slice(kb);
            pos += kb.len();
            self.raw[pos..pos + 8].copy_from_slice(&r.to_le_bytes());
            pos += 8;
        }
        self.raw[4..12].copy_from_slice(&self.next_leaf.to_le_bytes());
    }

    fn entries(&self) -> Vec<(Vec<u8>, u64)> {
        let mut entries = Vec::new();
        let mut pos = self.data_offset();
        for _ in 0..self.num_entries() {
            if pos + 2 > PAGE_SIZE {
                break;
            }
            let klen = u16::from_le_bytes(self.raw[pos..pos + 2].try_into().unwrap()) as usize;
            pos += 2;
            if pos + klen + 8 > PAGE_SIZE {
                break;
            }
            let key = self.raw[pos..pos + klen].to_vec();
            pos += klen;
            let rid = u64::from_le_bytes(self.raw[pos..pos + 8].try_into().unwrap());
            pos += 8;
            entries.push((key, rid));
        }
        entries
    }

    fn find_raw(&self, key_bytes: &[u8]) -> Vec<u64> {
        self.entries()
            .iter()
            .filter(|(k, _)| k[..] == key_bytes[..])
            .map(|(_, r)| *r)
            .collect()
    }

    fn split_raw(&self) -> (Vec<(Vec<u8>, u64)>, Vec<(Vec<u8>, u64)>, Vec<u8>) {
        let entries = self.entries();
        let mid = entries.len() / 2;
        let left = entries[..mid].to_vec();
        let right = entries[mid..].to_vec();
        let sep = right[0].0.clone();
        (left, right, sep)
    }
}

struct InternalPage {
    page_id: PageId,
    raw: [u8; PAGE_SIZE],
    rightmost_child: u64,
}

impl InternalPage {
    fn new(page_id: PageId) -> Self {
        let mut raw = [0u8; PAGE_SIZE];
        raw[0..2].copy_from_slice(&INTERNAL_MAGIC.to_le_bytes());
        Self {
            page_id,
            raw,
            rightmost_child: 0,
        }
    }

    fn from_raw(page_id: PageId, raw: [u8; PAGE_SIZE]) -> Self {
        let rightmost_child = u64::from_le_bytes(raw[4..12].try_into().unwrap());
        Self {
            page_id,
            raw,
            rightmost_child,
        }
    }

    fn num_keys(&self) -> usize {
        u16::from_le_bytes(self.raw[2..4].try_into().unwrap()) as usize
    }

    fn set_num_keys(&mut self, n: usize) {
        self.raw[2..4].copy_from_slice(&(n as u16).to_le_bytes());
    }

    fn find_child_bytes(&self, key_bytes: &[u8]) -> PageId {
        let mut pos = 12;
        let mut child = self.rightmost_child;
        for _ in 0..self.num_keys() {
            if pos + 2 > PAGE_SIZE {
                break;
            }
            let klen = u16::from_le_bytes(self.raw[pos..pos + 2].try_into().unwrap()) as usize;
            pos += 2;
            if pos + klen + 8 > PAGE_SIZE {
                break;
            }
            let k = &self.raw[pos..pos + klen];
            pos += klen;
            let c = u64::from_le_bytes(self.raw[pos..pos + 8].try_into().unwrap());
            pos += 8;
            if key_bytes[..] < k[..] {
                child = c;
                break;
            }
        }
        PageId(child)
    }

    fn insert_key_raw(&mut self, key_bytes: &[u8], left_child: u64) {
        let mut entries: Vec<(Vec<u8>, u64)> = Vec::new();
        let mut pos = 12;
        for _ in 0..self.num_keys() {
            if pos + 2 > PAGE_SIZE {
                break;
            }
            let klen = u16::from_le_bytes(self.raw[pos..pos + 2].try_into().unwrap()) as usize;
            pos += 2;
            if pos + klen + 8 > PAGE_SIZE {
                break;
            }
            let k = self.raw[pos..pos + klen].to_vec();
            pos += klen;
            let c = u64::from_le_bytes(self.raw[pos..pos + 8].try_into().unwrap());
            pos += 8;
            entries.push((k, c));
        }

        let ins_pos = entries.partition_point(|(k, _)| k[..] < key_bytes[..]);
        entries.insert(ins_pos, (key_bytes.to_vec(), left_child));

        self.set_num_keys(entries.len());
        pos = 12;
        for (k, c) in &entries {
            self.raw[pos..pos + 2].copy_from_slice(&(k.len() as u16).to_le_bytes());
            pos += 2;
            self.raw[pos..pos + k.len()].copy_from_slice(k);
            pos += k.len();
            self.raw[pos..pos + 8].copy_from_slice(&c.to_le_bytes());
            pos += 8;
        }
        self.raw[4..12].copy_from_slice(&self.rightmost_child.to_le_bytes());
    }

    fn all_children(&self) -> Vec<u64> {
        let mut children = Vec::new();
        let mut pos = 12;
        for _ in 0..self.num_keys() {
            if pos + 2 > PAGE_SIZE {
                break;
            }
            let klen = u16::from_le_bytes(self.raw[pos..pos + 2].try_into().unwrap()) as usize;
            pos += 2;
            if pos + klen + 8 > PAGE_SIZE {
                break;
            }
            pos += klen;
            let c = u64::from_le_bytes(self.raw[pos..pos + 8].try_into().unwrap());
            pos += 8;
            children.push(c);
        }
        children.push(self.rightmost_child);
        children
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_pager() -> (Arc<Mutex<Pager>>, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("minidb_btree_test_{}.db", std::process::id()));
        let _ = fs::remove_file(&path);
        let pager = Pager::open(&path).unwrap();
        (Arc::new(Mutex::new(pager)), path)
    }

    #[test]
    fn test_btree_insert_search() {
        let (pager, path) = setup_pager();
        let (idx, _) = BTreeIndex::create(1, &mut pager.clone()).unwrap();
        let mut wal_path = path.clone();
        wal_path.set_extension("wal");
        let mut wal = Wal::open(&wal_path).unwrap();

        for i in 0..100i64 {
            idx.insert(&Value::Int(i), i as u64, &mut wal, 0, 1)
                .unwrap();
        }

        let results = idx.search(&Value::Int(42)).unwrap();
        assert!(results.contains(&42u64));

        let range_results = idx
            .search_range(Some(&Value::Int(10)), Some(&Value::Int(19)))
            .unwrap();
        assert_eq!(range_results.len(), 10);

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&wal_path);
    }
}

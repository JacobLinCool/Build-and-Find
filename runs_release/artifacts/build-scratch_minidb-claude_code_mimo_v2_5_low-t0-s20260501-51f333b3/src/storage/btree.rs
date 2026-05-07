use crate::error::{MiniDbError, Result};
use crate::storage::heap::HeapFile;
use crate::storage::page::{Page, PageId, PAGE_SIZE};
use std::cmp::Ordering;
use std::collections::HashMap;

/// B-tree key: (index_key_bytes, row_id_page, row_id_slot)
/// For simplicity, keys are stored as Vec<u8> (serialized values)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BTreeKey {
    pub key: Vec<u8>,
    pub page_id: u64,
    pub slot: u16,
}

impl Ord for BTreeKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key
            .cmp(&other.key)
            .then(self.page_id.cmp(&other.page_id))
            .then(self.slot.cmp(&other.slot))
    }
}

impl PartialOrd for BTreeKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// B-tree node layout (simplified for secondary index):
/// Page layout:
///   [0]         is_leaf (u8): 1=leaf, 0=internal
///   [1..3]      num_keys (u16)
///   [3..5]      next_leaf (u16): page_id of next leaf (0 if none)
///   [5..]       keys and child pointers
///
/// Leaf node stores: keys + (page_id, slot) pairs
/// Internal node stores: keys + child page_ids

const HEADER_SIZE: usize = 5;
const MAX_KEYS: usize = (PAGE_SIZE - HEADER_SIZE) / 20; // approx

#[derive(Debug, Clone)]
pub struct BTreeIndex {
    root_page: u64,
    pages: HashMap<u64, Page>,
    key_serializer: fn(&crate::types::Value) -> Vec<u8>,
}

impl BTreeIndex {
    pub fn new(root_page: u64) -> Self {
        BTreeIndex {
            root_page,
            pages: HashMap::new(),
            key_serializer: serialize_value_for_index,
        }
    }

    pub fn init_root(&mut self, heap: &mut HeapFile) -> Result<()> {
        let mut page = Page::new(PageId(self.root_page));
        page.data[0] = 1; // leaf
        page.write_u16(1, 0); // num_keys = 0
        page.write_u16(3, 0); // next_leaf = 0
        heap.write_page(&page)?;
        self.pages.insert(self.root_page, page);
        Ok(())
    }

    pub fn insert(
        &mut self,
        key: Vec<u8>,
        page_id: u64,
        slot: u16,
        heap: &mut HeapFile,
    ) -> Result<()> {
        let btree_key = BTreeKey { key, page_id, slot };

        // Load root
        let root = self.load_node(self.root_page, heap)?;
        let num_keys = root.read_u16(1) as usize;

        if num_keys < MAX_KEYS {
            // Simple case: just insert into root leaf
            self.insert_into_leaf(self.root_page, btree_key, heap)?;
        } else {
            // Need to split
            self.insert_and_split(self.root_page, btree_key, heap)?;
        }

        Ok(())
    }

    fn insert_into_leaf(&mut self, page_id: u64, key: BTreeKey, heap: &mut HeapFile) -> Result<()> {
        let mut page = self.load_node(page_id, heap)?;
        let num_keys = page.read_u16(1) as usize;

        // Find insertion point
        let mut insert_pos = num_keys;
        for i in 0..num_keys {
            let existing = self.read_key(&page, i)?;
            if key.key < existing.key {
                insert_pos = i;
                break;
            }
        }

        // Shift keys to make room
        for i in (insert_pos..num_keys).rev() {
            self.shift_key(&mut page, i, i + 1)?;
        }

        // Write new key
        self.write_key(&mut page, insert_pos, &key)?;
        page.write_u16(1, (num_keys + 1) as u16);

        self.pages.insert(page_id, page.clone());
        heap.write_page(&page)?;
        Ok(())
    }

    fn insert_and_split(&mut self, page_id: u64, key: BTreeKey, heap: &mut HeapFile) -> Result<()> {
        // For simplicity, split the leaf and propagate up
        let page = self.load_node(page_id, heap)?;
        let num_keys = page.read_u16(1) as usize;

        // Collect all keys
        let mut all_keys = Vec::new();
        for i in 0..num_keys {
            all_keys.push(self.read_key(&page, i)?);
        }
        all_keys.push(key);
        all_keys.sort();

        // Split in half
        let mid = all_keys.len() / 2;
        let right_keys = all_keys.split_off(mid);

        // Update left page
        let mut left_page = page.clone();
        left_page.write_u16(1, all_keys.len() as u16);
        for (i, k) in all_keys.iter().enumerate() {
            self.write_key(&mut left_page, i, k)?;
        }

        // Create right page
        let right_page_id = heap.load_page(u64::MAX).map(|p| p.id.0 + 1).unwrap_or(1);
        let mut right_page = Page::new(PageId(right_page_id));
        right_page.data[0] = 1; // leaf
        right_page.write_u16(1, right_keys.len() as u16);
        for (i, k) in right_keys.iter().enumerate() {
            self.write_key(&mut right_page, i, k)?;
        }

        // Link leaves
        left_page.write_u16(3, right_page_id as u16);

        self.pages.insert(page_id, left_page.clone());
        self.pages.insert(right_page_id, right_page.clone());
        heap.write_page(&left_page)?;
        heap.write_page(&right_page)?;

        // If this is the root, create a new root
        if page_id == self.root_page {
            let new_root_id = heap.load_page(u64::MAX).map(|p| p.id.0 + 1).unwrap_or(0);
            let mut new_root = Page::new(PageId(new_root_id));
            new_root.data[0] = 0; // internal
            new_root.write_u16(1, 1); // one key

            // Write separator key and child pointers
            if let Some(median) = right_keys.first() {
                self.write_key(&mut new_root, 0, median)?;
            }
            // Store child page IDs after the keys
            let child_offset = HEADER_SIZE + MAX_KEYS * 20;
            new_root.write_u64(child_offset, page_id);
            new_root.write_u64(child_offset + 8, right_page_id);

            self.root_page = new_root_id;
            self.pages.insert(new_root_id, new_root.clone());
            heap.write_page(&new_root)?;
        }

        Ok(())
    }

    fn load_node(&mut self, page_id: u64, heap: &mut HeapFile) -> Result<Page> {
        if let Some(page) = self.pages.get(&page_id) {
            return Ok(page.clone());
        }
        let page = heap.load_page(page_id)?;
        self.pages.insert(page_id, page.clone());
        Ok(page)
    }

    fn read_key(&self, page: &Page, index: usize) -> Result<BTreeKey> {
        let offset = HEADER_SIZE + index * 20;
        let key_len = page.read_u16(offset) as usize;
        let key_start = offset + 2;
        let key: Vec<u8> = page.read_bytes(key_start, key_len).to_vec();
        let page_id_offset = key_start + key_len;
        let page_id = page.read_u64(page_id_offset);
        let slot = page.read_u16(page_id_offset + 8);
        Ok(BTreeKey { key, page_id, slot })
    }

    fn write_key(&self, page: &mut Page, index: usize, key: &BTreeKey) -> Result<()> {
        let offset = HEADER_SIZE + index * 20;
        let key_len = key.key.len().min(10) as u16;
        page.write_u16(offset, key_len);
        let key_start = offset + 2;
        page.write_bytes(key_start, &key.key[..key_len as usize]);
        let page_id_offset = key_start + key_len as usize;
        page.write_u64(page_id_offset, key.page_id);
        page.write_u16(page_id_offset + 8, key.slot);
        Ok(())
    }

    fn shift_key(&self, page: &mut Page, from: usize, to: usize) -> Result<()> {
        let key = self.read_key(page, from)?;
        self.write_key(page, to, &key)?;
        Ok(())
    }

    pub fn search(&mut self, key: &[u8], heap: &mut HeapFile) -> Result<Vec<(u64, u16)>> {
        let mut results = Vec::new();
        let mut page_id = self.root_page;

        loop {
            let page = self.load_node(page_id, heap)?;
            let is_leaf = page.data[0] == 1;
            let num_keys = page.read_u16(1) as usize;

            if is_leaf {
                for i in 0..num_keys {
                    let k = self.read_key(&page, i)?;
                    if k.key == key {
                        results.push((k.page_id, k.slot));
                    }
                }
                break;
            } else {
                // Internal node: find child to descend
                let mut found = false;
                for i in 0..num_keys {
                    let k = self.read_key(&page, i)?;
                    if key <= &k.key {
                        let child_offset = HEADER_SIZE + MAX_KEYS * 20 + i * 8;
                        page_id = page.read_u64(child_offset);
                        found = true;
                        break;
                    }
                }
                if !found {
                    let child_offset = HEADER_SIZE + MAX_KEYS * 20 + num_keys * 8;
                    page_id = page.read_u64(child_offset);
                }
            }
        }

        Ok(results)
    }

    pub fn range_search(
        &mut self,
        low: &[u8],
        high: &[u8],
        heap: &mut HeapFile,
    ) -> Result<Vec<(u64, u16)>> {
        let mut results = Vec::new();
        let mut page_id = self.root_page;

        // Find the leftmost leaf
        loop {
            let page = self.load_node(page_id, heap)?;
            let is_leaf = page.data[0] == 1;
            if is_leaf {
                break;
            }
            let child_offset = HEADER_SIZE + MAX_KEYS * 20;
            page_id = page.read_u64(child_offset);
        }

        // Scan leaves
        loop {
            let page = self.load_node(page_id, heap)?;
            let num_keys = page.read_u16(1) as usize;

            for i in 0..num_keys {
                let k = self.read_key(&page, i)?;
                if k.key >= low.to_vec() && k.key <= high.to_vec() {
                    results.push((k.page_id, k.slot));
                }
            }

            let next = page.read_u16(3) as u64;
            if next == 0 {
                break;
            }
            page_id = next;
        }

        Ok(results)
    }
}

pub fn serialize_value_for_index(val: &crate::types::Value) -> Vec<u8> {
    match val {
        crate::types::Value::Int(v) => v.to_le_bytes().to_vec(),
        crate::types::Value::Real(v) => v.to_le_bytes().to_vec(),
        crate::types::Value::Text(v) => v.as_bytes().to_vec(),
        crate::types::Value::Bool(v) => vec![*v as u8],
        crate::types::Value::Null => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_btree_insert_and_search() {
        let dir = tempdir().unwrap();
        let heap_path = dir.path().join("test.heap");

        let mut heap = HeapFile::open(&heap_path).unwrap();
        let root_page = 0;

        let page = Page::new(PageId(root_page));
        heap.write_page(&page).unwrap();

        let mut btree = BTreeIndex::new(root_page);
        btree.init_root(&mut heap).unwrap();

        // Insert some keys
        for i in 0..10u8 {
            let key = vec![i];
            btree.insert(key, 0, i as u16, &mut heap).unwrap();
        }

        // Search
        let results = btree.search(&[5u8], &mut heap).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, 5);
    }
}

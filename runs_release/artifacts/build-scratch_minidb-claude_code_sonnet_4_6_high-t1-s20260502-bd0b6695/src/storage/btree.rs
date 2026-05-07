/// B+ tree secondary index implementation.
/// Keys are (encoded_value_bytes, row_id) for uniqueness.
/// Supports equality and range lookups.
use crate::error::{MiniDbError, Result};
use crate::storage::heap::RowId;
use crate::storage::page::{
    Page, PageId, PageManager, PAGE_TYPE_BTREE_INTERNAL, PAGE_TYPE_BTREE_LEAF,
};
use crate::types::Value;

// B-tree constants
const MAX_LEAF_ENTRIES: usize = 30;
const MAX_INTERNAL_KEYS: usize = 30; // max children = MAX_INTERNAL_KEYS + 1

/// A key in the BTree: (value_key_bytes, row_id).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BTreeKey {
    pub key_bytes: Vec<u8>,
    pub row_id: u64,
}

impl BTreeKey {
    pub fn new(value: &Value, row_id: RowId) -> Self {
        BTreeKey {
            key_bytes: value.encode_key(),
            row_id: row_id.encode(),
        }
    }

    /// Key prefix for range comparisons (just the value portion).
    pub fn from_prefix(value: &Value) -> Self {
        BTreeKey {
            key_bytes: value.encode_key(),
            row_id: 0,
        }
    }

    pub fn from_prefix_max(value: &Value) -> Self {
        BTreeKey {
            key_bytes: value.encode_key(),
            row_id: u64::MAX,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(self.key_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(&self.key_bytes);
        buf.extend_from_slice(&self.row_id.to_le_bytes());
        buf
    }

    fn decode(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() < 2 {
            return Err(MiniDbError::StorageCorruption("btree key too short".into()));
        }
        let klen = u16::from_le_bytes([data[0], data[1]]) as usize;
        if data.len() < 2 + klen + 8 {
            return Err(MiniDbError::StorageCorruption("btree key truncated".into()));
        }
        let key_bytes = data[2..2 + klen].to_vec();
        let row_id = u64::from_le_bytes(data[2 + klen..2 + klen + 8].try_into().unwrap());
        Ok((BTreeKey { key_bytes, row_id }, 2 + klen + 8))
    }
}

// ---- Page-level BTree node serialization ----

struct LeafNode {
    entries: Vec<BTreeKey>,
    next: PageId,
}

impl LeafNode {
    fn new() -> Self {
        LeafNode {
            entries: Vec::new(),
            next: PageId::null(),
        }
    }

    fn from_page(page: &Page) -> Result<Self> {
        // Layout: [num_entries: u16][next_page: u8*8][entries...]
        let data = &page.data[1..]; // skip page_type byte
        let num_entries = u16::from_le_bytes([data[0], data[1]]) as usize;
        let next = PageId(u64::from_le_bytes(data[2..10].try_into().unwrap()));
        let mut pos = 10;
        let mut entries = Vec::with_capacity(num_entries);
        for _ in 0..num_entries {
            let (key, consumed) = BTreeKey::decode(&data[pos..])?;
            entries.push(key);
            pos += consumed;
        }
        Ok(LeafNode { entries, next })
    }

    fn to_page(&self) -> Page {
        let mut page = Page::new();
        page.data[0] = PAGE_TYPE_BTREE_LEAF;
        let num = self.entries.len() as u16;
        page.data[1..3].copy_from_slice(&num.to_le_bytes());
        page.data[3..11].copy_from_slice(&self.next.0.to_le_bytes());
        let mut pos = 11;
        for entry in &self.entries {
            let enc = entry.encode();
            page.data[pos..pos + enc.len()].copy_from_slice(&enc);
            pos += enc.len();
        }
        page
    }

    fn is_full(&self) -> bool {
        self.entries.len() >= MAX_LEAF_ENTRIES
    }
}

struct InternalNode {
    keys: Vec<BTreeKey>,
    children: Vec<PageId>,
}

impl InternalNode {
    fn new() -> Self {
        InternalNode {
            keys: Vec::new(),
            children: Vec::new(),
        }
    }

    fn from_page(page: &Page) -> Result<Self> {
        let data = &page.data[1..];
        let num_keys = u16::from_le_bytes([data[0], data[1]]) as usize;
        let mut pos = 2;
        let mut children = Vec::with_capacity(num_keys + 1);
        let mut keys = Vec::with_capacity(num_keys);

        // First child
        if pos + 8 > data.len() {
            return Ok(InternalNode::new());
        }
        let child0 = PageId(u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap()));
        children.push(child0);
        pos += 8;

        for _ in 0..num_keys {
            let (key, consumed) = BTreeKey::decode(&data[pos..])?;
            keys.push(key);
            pos += consumed;
            if pos + 8 <= data.len() {
                let child = PageId(u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap()));
                children.push(child);
                pos += 8;
            }
        }

        Ok(InternalNode { keys, children })
    }

    fn to_page(&self) -> Page {
        let mut page = Page::new();
        page.data[0] = PAGE_TYPE_BTREE_INTERNAL;
        let nk = self.keys.len() as u16;
        page.data[1..3].copy_from_slice(&nk.to_le_bytes());
        let mut pos = 3;
        for (i, child) in self.children.iter().enumerate() {
            page.data[pos..pos + 8].copy_from_slice(&child.0.to_le_bytes());
            pos += 8;
            if i < self.keys.len() {
                let enc = self.keys[i].encode();
                page.data[pos..pos + enc.len()].copy_from_slice(&enc);
                pos += enc.len();
            }
        }
        page
    }

    fn is_full(&self) -> bool {
        self.keys.len() >= MAX_INTERNAL_KEYS
    }
}

/// B+ tree index. root_page == PageId::null() means empty tree.
pub struct BTreeIndex {
    pub root: PageId,
}

impl BTreeIndex {
    pub fn new() -> Self {
        BTreeIndex {
            root: PageId::null(),
        }
    }

    /// Insert a (value, row_id) pair into the index.
    pub fn insert(&mut self, pm: &mut PageManager, value: &Value, row_id: RowId) -> Result<()> {
        let key = BTreeKey::new(value, row_id);

        if self.root.is_null() {
            // Create root leaf
            let mut leaf = LeafNode::new();
            leaf.entries.push(key);
            let page_id = pm.alloc_page()?;
            pm.write_page(page_id, &leaf.to_page())?;
            self.root = page_id;
            return Ok(());
        }

        let result = self.insert_recursive(pm, self.root, &key)?;
        if let Some((split_key, right_page)) = result {
            // Root split: create new root
            let mut new_root = InternalNode::new();
            new_root.keys.push(split_key);
            new_root.children.push(self.root);
            new_root.children.push(right_page);
            let new_root_id = pm.alloc_page()?;
            pm.write_page(new_root_id, &new_root.to_page())?;
            self.root = new_root_id;
        }
        Ok(())
    }

    fn insert_recursive(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        key: &BTreeKey,
    ) -> Result<Option<(BTreeKey, PageId)>> {
        let page = pm.read_page(page_id)?;
        match page.data[0] {
            PAGE_TYPE_BTREE_LEAF => {
                let mut leaf = LeafNode::from_page(&page)?;
                let pos = leaf.entries.partition_point(|k| k < key);
                leaf.entries.insert(pos, key.clone());

                if !leaf.is_full() {
                    pm.write_page(page_id, &leaf.to_page())?;
                    Ok(None)
                } else {
                    // Split leaf
                    let mid = leaf.entries.len() / 2;
                    let right_entries = leaf.entries.split_off(mid);
                    let split_key = right_entries[0].clone();

                    let right_page_id = pm.alloc_page()?;
                    let mut right_leaf = LeafNode::new();
                    right_leaf.entries = right_entries;
                    right_leaf.next = leaf.next;
                    leaf.next = right_page_id;

                    pm.write_page(page_id, &leaf.to_page())?;
                    pm.write_page(right_page_id, &right_leaf.to_page())?;
                    Ok(Some((split_key, right_page_id)))
                }
            }
            PAGE_TYPE_BTREE_INTERNAL => {
                let mut node = InternalNode::from_page(&page)?;
                let pos = node.keys.partition_point(|k| k <= key);
                let child = node.children[pos];

                let result = self.insert_recursive(pm, child, key)?;
                if let Some((split_key, right_child)) = result {
                    node.keys.insert(pos, split_key);
                    node.children.insert(pos + 1, right_child);

                    if !node.is_full() {
                        pm.write_page(page_id, &node.to_page())?;
                        Ok(None)
                    } else {
                        // Split internal node
                        let mid = node.keys.len() / 2;
                        let promoted = node.keys[mid].clone();
                        let right_keys = node.keys.split_off(mid + 1);
                        node.keys.pop(); // remove the promoted key
                        let right_children = node.children.split_off(mid + 1);

                        let mut right_node = InternalNode::new();
                        right_node.keys = right_keys;
                        right_node.children = right_children;

                        let right_id = pm.alloc_page()?;
                        pm.write_page(page_id, &node.to_page())?;
                        pm.write_page(right_id, &right_node.to_page())?;
                        Ok(Some((promoted, right_id)))
                    }
                } else {
                    pm.write_page(page_id, &node.to_page())?;
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    /// Delete a (value, row_id) pair from the index.
    pub fn delete(&mut self, pm: &mut PageManager, value: &Value, row_id: RowId) -> Result<bool> {
        if self.root.is_null() {
            return Ok(false);
        }
        let key = BTreeKey::new(value, row_id);
        self.delete_from_leaf(pm, self.root, &key)
    }

    fn delete_from_leaf(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        key: &BTreeKey,
    ) -> Result<bool> {
        let page = pm.read_page(page_id)?;
        match page.data[0] {
            PAGE_TYPE_BTREE_LEAF => {
                let mut leaf = LeafNode::from_page(&page)?;
                if let Some(pos) = leaf.entries.iter().position(|k| k == key) {
                    leaf.entries.remove(pos);
                    pm.write_page(page_id, &leaf.to_page())?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            PAGE_TYPE_BTREE_INTERNAL => {
                let node = InternalNode::from_page(&page)?;
                let pos = node.keys.partition_point(|k| k <= key);
                let child = node.children[pos];
                self.delete_from_leaf(pm, child, key)
            }
            _ => Ok(false),
        }
    }

    /// Find the leftmost leaf page for range scan starting at `low_key`.
    fn find_leaf_ge(&self, pm: &mut PageManager, low_key: &BTreeKey) -> Result<Option<PageId>> {
        if self.root.is_null() {
            return Ok(None);
        }
        let mut page_id = self.root;
        loop {
            let page = pm.read_page(page_id)?;
            match page.data[0] {
                PAGE_TYPE_BTREE_LEAF => return Ok(Some(page_id)),
                PAGE_TYPE_BTREE_INTERNAL => {
                    let node = InternalNode::from_page(&page)?;
                    let pos = node.keys.partition_point(|k| k <= low_key);
                    let pos = pos.min(node.children.len().saturating_sub(1));
                    page_id = node.children[pos];
                }
                _ => return Ok(None),
            }
        }
    }

    /// Range scan: returns all RowIds where low <= key <= high (inclusive).
    pub fn range_scan(
        &self,
        pm: &mut PageManager,
        low: Option<&Value>,
        high: Option<&Value>,
    ) -> Result<Vec<RowId>> {
        let low_key = low.map(BTreeKey::from_prefix);
        let high_key = high.map(BTreeKey::from_prefix_max);

        let start_page = if let Some(ref lk) = low_key {
            self.find_leaf_ge(pm, lk)?
        } else {
            self.find_leftmost_leaf(pm)?
        };

        let mut results = Vec::new();
        let mut page_id = match start_page {
            None => return Ok(results),
            Some(p) => p,
        };

        loop {
            let page = pm.read_page(page_id)?;
            if page.data[0] != PAGE_TYPE_BTREE_LEAF {
                break;
            }
            let leaf = LeafNode::from_page(&page)?;

            for entry in &leaf.entries {
                if let Some(ref lk) = low_key {
                    if entry < lk {
                        continue;
                    }
                }
                if let Some(ref hk) = high_key {
                    if entry > hk {
                        return Ok(results);
                    }
                }
                results.push(RowId::decode(entry.row_id));
            }

            page_id = leaf.next;
            if page_id.is_null() {
                break;
            }
        }
        Ok(results)
    }

    /// Equality scan: all RowIds with key equal to `value`.
    pub fn eq_scan(&self, pm: &mut PageManager, value: &Value) -> Result<Vec<RowId>> {
        self.range_scan(pm, Some(value), Some(value))
    }

    fn find_leftmost_leaf(&self, pm: &mut PageManager) -> Result<Option<PageId>> {
        if self.root.is_null() {
            return Ok(None);
        }
        let mut page_id = self.root;
        loop {
            let page = pm.read_page(page_id)?;
            match page.data[0] {
                PAGE_TYPE_BTREE_LEAF => return Ok(Some(page_id)),
                PAGE_TYPE_BTREE_INTERNAL => {
                    let node = InternalNode::from_page(&page)?;
                    if node.children.is_empty() {
                        return Ok(None);
                    }
                    page_id = node.children[0];
                }
                _ => return Ok(None),
            }
        }
    }
}

impl Default for BTreeIndex {
    fn default() -> Self {
        Self::new()
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

    fn rid(n: u64) -> RowId {
        RowId::decode(n << 16)
    }

    #[test]
    fn test_btree_insert_eq_scan() {
        let (mut pm, _tmp) = make_pm();
        let mut idx = BTreeIndex::new();

        idx.insert(&mut pm, &Value::Int(10), rid(1)).unwrap();
        idx.insert(&mut pm, &Value::Int(20), rid(2)).unwrap();
        idx.insert(&mut pm, &Value::Int(10), rid(3)).unwrap();

        let results = idx.eq_scan(&mut pm, &Value::Int(10)).unwrap();
        assert_eq!(results.len(), 2);

        let results2 = idx.eq_scan(&mut pm, &Value::Int(20)).unwrap();
        assert_eq!(results2.len(), 1);
    }

    #[test]
    fn test_btree_range_scan() {
        let (mut pm, _tmp) = make_pm();
        let mut idx = BTreeIndex::new();

        for i in 1..=10 {
            idx.insert(&mut pm, &Value::Int(i), rid(i as u64)).unwrap();
        }

        let results = idx
            .range_scan(&mut pm, Some(&Value::Int(3)), Some(&Value::Int(7)))
            .unwrap();
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_btree_delete() {
        let (mut pm, _tmp) = make_pm();
        let mut idx = BTreeIndex::new();

        let r = rid(1);
        idx.insert(&mut pm, &Value::Int(42), r).unwrap();
        idx.delete(&mut pm, &Value::Int(42), r).unwrap();

        let results = idx.eq_scan(&mut pm, &Value::Int(42)).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_btree_many_inserts() {
        let (mut pm, _tmp) = make_pm();
        let mut idx = BTreeIndex::new();

        // Insert enough to cause splits
        for i in 0..100 {
            idx.insert(&mut pm, &Value::Int(i), rid(i as u64)).unwrap();
        }

        let all = idx.range_scan(&mut pm, None, None).unwrap();
        assert_eq!(all.len(), 100);
    }
}

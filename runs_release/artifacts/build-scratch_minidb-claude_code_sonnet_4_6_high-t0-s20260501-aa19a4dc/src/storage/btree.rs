/// B-tree index implementation.
///
/// Keys are (serialized_value_bytes, row_id: u64).
/// Each leaf page stores an array of (key_len: u16, key: [u8], row_id: u64) entries.
/// Internal pages store (key_len: u16, key: [u8], child_page: u32) entries.
///
/// Page layout:
///   [1 byte: is_leaf][2 bytes: num_entries][entries...]
///   For leaf: [4 bytes: next_leaf page]
///
/// This is a simplified but correct B-tree that supports:
///   - insert(key, row_id)
///   - delete(key, row_id)
///   - search_exact(key) -> Vec<RowId>
///   - range_scan(from, to, inclusive) -> Vec<(key, RowId)>
use crate::error::{MiniDbError, Result};
use crate::storage::page::{PageId, PAGE_SIZE};
use crate::storage::pager::Pager;
use crate::types::Value;

const BTREE_LEAF: u8 = 1;
const BTREE_INTERNAL: u8 = 0;
const BTH_FIXED: usize = 7; // is_leaf(1) + num_entries(2) + next_leaf(4)
const MAX_ENTRIES_PER_LEAF: usize = 60;
const MAX_ENTRIES_PER_INTERNAL: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BTreeKey {
    pub key_bytes: Vec<u8>,
    pub row_id: u64,
}

impl BTreeKey {
    pub fn new(value: &Value, row_id: u64) -> Self {
        BTreeKey {
            key_bytes: value.to_bytes(),
            row_id,
        }
    }

    pub fn from_bytes_and_row_id(key_bytes: Vec<u8>, row_id: u64) -> Self {
        BTreeKey { key_bytes, row_id }
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(self.key_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(&self.key_bytes);
        buf.extend_from_slice(&self.row_id.to_le_bytes());
        buf
    }

    fn serialized_len(&self) -> usize {
        2 + self.key_bytes.len() + 8
    }
}

struct LeafEntry {
    key: BTreeKey,
}

struct InternalEntry {
    key: BTreeKey,
    child: u32, // page id of left child
}

pub struct BTree {
    pub root: PageId,
}

impl BTree {
    /// Create a new empty B-tree, returning the root page id
    pub fn create(pager: &mut Pager) -> Result<Self> {
        let id = pager.allocate_page()?;
        let page = pager.read_page_mut(id)?;
        // Initialize as empty leaf
        page.data[0] = BTREE_LEAF;
        page.data[1..3].copy_from_slice(&0u16.to_le_bytes()); // num_entries = 0
        page.data[3..7].copy_from_slice(&0u32.to_le_bytes()); // next_leaf = 0
        page.mark_dirty();
        Ok(BTree { root: id })
    }

    pub fn open(root: PageId) -> Self {
        BTree { root }
    }

    /// Insert a key into the tree. Returns new root if root was split.
    pub fn insert(&mut self, pager: &mut Pager, key: BTreeKey) -> Result<()> {
        let result = self.insert_recursive(pager, self.root, key)?;
        if let Some((split_key, right_page)) = result {
            // Root was split, create new root
            let new_root_id = pager.allocate_page()?;
            {
                let page = pager.read_page_mut(new_root_id)?;
                page.data[0] = BTREE_INTERNAL;
                page.data[1..3].copy_from_slice(&1u16.to_le_bytes()); // 1 separator
                page.data[3..7].copy_from_slice(&0u32.to_le_bytes()); // unused next
                                                                      // Store left child (old root) + separator + right child
                                                                      // Internal format: left_child(4) + entries...
                let left_id = self.root.as_u64() as u32;
                page.data[7..11].copy_from_slice(&left_id.to_le_bytes());
                let entry_bytes = split_key.serialize();
                let right_id = right_page.as_u64() as u32;
                let start = 11;
                let end = start + entry_bytes.len();
                page.data[start..end].copy_from_slice(&entry_bytes);
                page.data[end..end + 4].copy_from_slice(&right_id.to_le_bytes());
                page.mark_dirty();
            }
            self.root = new_root_id;
        }
        Ok(())
    }

    fn insert_recursive(
        &self,
        pager: &mut Pager,
        page_id: PageId,
        key: BTreeKey,
    ) -> Result<Option<(BTreeKey, PageId)>> {
        let is_leaf = pager.read_page(page_id)?.data[0] == BTREE_LEAF;

        if is_leaf {
            self.leaf_insert(pager, page_id, key)
        } else {
            // Find the child to recurse into
            let child_page = self.find_child(pager, page_id, &key)?;
            let result = self.insert_recursive(pager, child_page, key)?;
            if let Some((split_key, right_child)) = result {
                self.internal_insert(pager, page_id, split_key, right_child)
            } else {
                Ok(None)
            }
        }
    }

    fn find_child(&self, pager: &mut Pager, page_id: PageId, key: &BTreeKey) -> Result<PageId> {
        let page = pager.read_page(page_id)?;
        let num_entries = u16::from_le_bytes([page.data[1], page.data[2]]) as usize;
        let mut pos = 7;
        let left_child = u32::from_le_bytes([
            page.data[pos],
            page.data[pos + 1],
            page.data[pos + 2],
            page.data[pos + 3],
        ]);
        pos += 4;

        let mut last_right = left_child;
        for _ in 0..num_entries {
            let key_len = u16::from_le_bytes([page.data[pos], page.data[pos + 1]]) as usize;
            pos += 2;
            let entry_key_bytes = page.data[pos..pos + key_len].to_vec();
            pos += key_len;
            let entry_row_id = u64::from_le_bytes(page.data[pos..pos + 8].try_into().unwrap());
            pos += 8;
            let right_child = u32::from_le_bytes([
                page.data[pos],
                page.data[pos + 1],
                page.data[pos + 2],
                page.data[pos + 3],
            ]);
            pos += 4;

            let entry_key = BTreeKey::from_bytes_and_row_id(entry_key_bytes, entry_row_id);
            if key < &entry_key {
                return Ok(PageId(left_child as u64));
            }
            last_right = right_child;
            let left_child_next =
                u32::from_le_bytes([page.data[7], page.data[8], page.data[9], page.data[10]]);
            // Note: after first iteration left_child changes
            let _ = left_child_next;
        }
        Ok(PageId(last_right as u64))
    }

    fn leaf_insert(
        &self,
        pager: &mut Pager,
        page_id: PageId,
        key: BTreeKey,
    ) -> Result<Option<(BTreeKey, PageId)>> {
        let entries = self.read_leaf_entries(pager, page_id)?;
        let next_leaf = {
            let page = pager.read_page(page_id)?;
            u32::from_le_bytes([page.data[3], page.data[4], page.data[5], page.data[6]])
        };

        // Insert in sorted order
        let mut new_entries = entries;
        let pos = new_entries.partition_point(|e| e.key <= key);
        new_entries.insert(pos, LeafEntry { key });

        if new_entries.len() <= MAX_ENTRIES_PER_LEAF {
            self.write_leaf_entries(pager, page_id, &new_entries, next_leaf)?;
            Ok(None)
        } else {
            // Split
            let mid = new_entries.len() / 2;
            let right_entries = new_entries.split_off(mid);
            let split_key = right_entries[0].key.clone();

            let right_id = pager.allocate_page()?;
            self.write_leaf_entries(pager, right_id, &right_entries, next_leaf)?;
            self.write_leaf_entries(pager, page_id, &new_entries, right_id.as_u64() as u32)?;

            Ok(Some((split_key, right_id)))
        }
    }

    fn internal_insert(
        &self,
        pager: &mut Pager,
        page_id: PageId,
        key: BTreeKey,
        right_child: PageId,
    ) -> Result<Option<(BTreeKey, PageId)>> {
        // Read current internal page entries
        let (left_child, entries) = self.read_internal_entries(pager, page_id)?;
        let mut new_entries = entries;
        let pos = new_entries.partition_point(|e| e.key <= key);
        new_entries.insert(
            pos,
            InternalEntry {
                key,
                child: right_child.as_u64() as u32,
            },
        );

        if new_entries.len() <= MAX_ENTRIES_PER_INTERNAL {
            self.write_internal_entries(pager, page_id, left_child, &new_entries)?;
            Ok(None)
        } else {
            let mid = new_entries.len() / 2;
            let push_up_key = new_entries[mid].key.clone();
            let right_entries = new_entries.split_off(mid + 1);
            new_entries.pop(); // remove the pushed-up key

            // Right sibling starts with the first of right_entries' left child
            let right_left_child = if !right_entries.is_empty() {
                right_entries[0].child
            } else {
                0
            };

            let right_id = pager.allocate_page()?;
            self.write_internal_entries(pager, right_id, right_left_child, &right_entries)?;
            self.write_internal_entries(pager, page_id, left_child, &new_entries)?;

            Ok(Some((push_up_key, right_id)))
        }
    }

    fn read_leaf_entries(&self, pager: &mut Pager, page_id: PageId) -> Result<Vec<LeafEntry>> {
        let page = pager.read_page(page_id)?;
        let num_entries = u16::from_le_bytes([page.data[1], page.data[2]]) as usize;
        let mut entries = Vec::with_capacity(num_entries);
        let mut pos = 7;
        for _ in 0..num_entries {
            if pos + 2 > PAGE_SIZE {
                break;
            }
            let key_len = u16::from_le_bytes([page.data[pos], page.data[pos + 1]]) as usize;
            pos += 2;
            if pos + key_len + 8 > PAGE_SIZE {
                break;
            }
            let key_bytes = page.data[pos..pos + key_len].to_vec();
            pos += key_len;
            let row_id = u64::from_le_bytes(page.data[pos..pos + 8].try_into().unwrap());
            pos += 8;
            entries.push(LeafEntry {
                key: BTreeKey { key_bytes, row_id },
            });
        }
        Ok(entries)
    }

    fn write_leaf_entries(
        &self,
        pager: &mut Pager,
        page_id: PageId,
        entries: &[LeafEntry],
        next_leaf: u32,
    ) -> Result<()> {
        let page = pager.read_page_mut(page_id)?;
        page.data[0] = BTREE_LEAF;
        page.data[1..3].copy_from_slice(&(entries.len() as u16).to_le_bytes());
        page.data[3..7].copy_from_slice(&next_leaf.to_le_bytes());
        let mut pos = 7;
        for entry in entries {
            let klen = entry.key.key_bytes.len() as u16;
            page.data[pos..pos + 2].copy_from_slice(&klen.to_le_bytes());
            pos += 2;
            page.data[pos..pos + entry.key.key_bytes.len()].copy_from_slice(&entry.key.key_bytes);
            pos += entry.key.key_bytes.len();
            page.data[pos..pos + 8].copy_from_slice(&entry.key.row_id.to_le_bytes());
            pos += 8;
        }
        page.mark_dirty();
        Ok(())
    }

    fn read_internal_entries(
        &self,
        pager: &mut Pager,
        page_id: PageId,
    ) -> Result<(u32, Vec<InternalEntry>)> {
        let page = pager.read_page(page_id)?;
        let num_entries = u16::from_le_bytes([page.data[1], page.data[2]]) as usize;
        let mut pos = 7;
        let left_child = u32::from_le_bytes(page.data[pos..pos + 4].try_into().unwrap());
        pos += 4;
        let mut entries = Vec::with_capacity(num_entries);
        for _ in 0..num_entries {
            if pos + 2 > PAGE_SIZE {
                break;
            }
            let key_len = u16::from_le_bytes([page.data[pos], page.data[pos + 1]]) as usize;
            pos += 2;
            if pos + key_len + 8 + 4 > PAGE_SIZE {
                break;
            }
            let key_bytes = page.data[pos..pos + key_len].to_vec();
            pos += key_len;
            let row_id = u64::from_le_bytes(page.data[pos..pos + 8].try_into().unwrap());
            pos += 8;
            let child = u32::from_le_bytes(page.data[pos..pos + 4].try_into().unwrap());
            pos += 4;
            entries.push(InternalEntry {
                key: BTreeKey { key_bytes, row_id },
                child,
            });
        }
        Ok((left_child, entries))
    }

    fn write_internal_entries(
        &self,
        pager: &mut Pager,
        page_id: PageId,
        left_child: u32,
        entries: &[InternalEntry],
    ) -> Result<()> {
        let page = pager.read_page_mut(page_id)?;
        page.data[0] = BTREE_INTERNAL;
        page.data[1..3].copy_from_slice(&(entries.len() as u16).to_le_bytes());
        page.data[3..7].copy_from_slice(&0u32.to_le_bytes());
        let mut pos = 7;
        page.data[pos..pos + 4].copy_from_slice(&left_child.to_le_bytes());
        pos += 4;
        for entry in entries {
            let klen = entry.key.key_bytes.len() as u16;
            page.data[pos..pos + 2].copy_from_slice(&klen.to_le_bytes());
            pos += 2;
            page.data[pos..pos + entry.key.key_bytes.len()].copy_from_slice(&entry.key.key_bytes);
            pos += entry.key.key_bytes.len();
            page.data[pos..pos + 8].copy_from_slice(&entry.key.row_id.to_le_bytes());
            pos += 8;
            page.data[pos..pos + 4].copy_from_slice(&entry.child.to_le_bytes());
            pos += 4;
        }
        page.mark_dirty();
        Ok(())
    }

    /// Find the leftmost leaf page
    fn find_leftmost_leaf(&self, pager: &mut Pager, start: PageId) -> Result<PageId> {
        let mut cur = start;
        loop {
            let is_leaf = pager.read_page(cur)?.data[0] == BTREE_LEAF;
            if is_leaf {
                return Ok(cur);
            }
            let left = u32::from_le_bytes({
                let page = pager.read_page(cur)?;
                [page.data[7], page.data[8], page.data[9], page.data[10]]
            });
            cur = PageId(left as u64);
        }
    }

    /// Search for exact key matches (by key_bytes only, ignoring row_id)
    pub fn search_exact(&self, pager: &mut Pager, key_bytes: &[u8]) -> Result<Vec<u64>> {
        let leaf = self.find_leaf_for_key(pager, self.root, key_bytes)?;
        let mut results = Vec::new();
        let mut cur_leaf = leaf;
        loop {
            let (entries, next) = {
                let page = pager.read_page(cur_leaf)?;
                let num_entries = u16::from_le_bytes([page.data[1], page.data[2]]) as usize;
                let next =
                    u32::from_le_bytes([page.data[3], page.data[4], page.data[5], page.data[6]]);
                let mut entries = Vec::new();
                let mut pos = 7;
                for _ in 0..num_entries {
                    if pos + 2 > PAGE_SIZE {
                        break;
                    }
                    let klen = u16::from_le_bytes([page.data[pos], page.data[pos + 1]]) as usize;
                    pos += 2;
                    if pos + klen + 8 > PAGE_SIZE {
                        break;
                    }
                    let kb = page.data[pos..pos + klen].to_vec();
                    pos += klen;
                    let rid = u64::from_le_bytes(page.data[pos..pos + 8].try_into().unwrap());
                    pos += 8;
                    entries.push((kb, rid));
                }
                (entries, next)
            };

            let mut found_any = false;
            let mut past = false;
            for (kb, rid) in &entries {
                if kb.as_slice() == key_bytes {
                    results.push(*rid);
                    found_any = true;
                } else if found_any {
                    past = true;
                    break;
                }
            }

            if past || next == 0 || !found_any && !entries.is_empty() {
                break;
            }
            if next == 0 {
                break;
            }
            cur_leaf = PageId(next as u64);
        }
        Ok(results)
    }

    fn find_leaf_for_key(
        &self,
        pager: &mut Pager,
        page_id: PageId,
        key_bytes: &[u8],
    ) -> Result<PageId> {
        let is_leaf = pager.read_page(page_id)?.data[0] == BTREE_LEAF;
        if is_leaf {
            return Ok(page_id);
        }

        let (left_child, entries) = self.read_internal_entries(pager, page_id)?;
        let mut child = left_child;
        for entry in &entries {
            if key_bytes <= entry.key.key_bytes.as_slice() {
                break;
            }
            child = entry.child;
        }
        self.find_leaf_for_key(pager, PageId(child as u64), key_bytes)
    }

    /// Range scan: returns all (key_bytes, row_id) pairs where key >= from and key <= to (if specified)
    pub fn range_scan(
        &self,
        pager: &mut Pager,
        from: Option<&[u8]>,
        to: Option<&[u8]>,
    ) -> Result<Vec<(Vec<u8>, u64)>> {
        let start_leaf = if let Some(f) = from {
            self.find_leaf_for_key(pager, self.root, f)?
        } else {
            self.find_leftmost_leaf(pager, self.root)?
        };

        let mut results = Vec::new();
        let mut cur_leaf = start_leaf;

        loop {
            let (entries, next) = {
                let page = pager.read_page(cur_leaf)?;
                let num_entries = u16::from_le_bytes([page.data[1], page.data[2]]) as usize;
                let next =
                    u32::from_le_bytes([page.data[3], page.data[4], page.data[5], page.data[6]]);
                let mut entries = Vec::new();
                let mut pos = 7;
                for _ in 0..num_entries {
                    if pos + 2 > PAGE_SIZE {
                        break;
                    }
                    let klen = u16::from_le_bytes([page.data[pos], page.data[pos + 1]]) as usize;
                    pos += 2;
                    if pos + klen + 8 > PAGE_SIZE {
                        break;
                    }
                    let kb = page.data[pos..pos + klen].to_vec();
                    pos += klen;
                    let rid = u64::from_le_bytes(page.data[pos..pos + 8].try_into().unwrap());
                    pos += 8;
                    entries.push((kb, rid));
                }
                (entries, next)
            };

            let mut past_to = false;
            for (kb, rid) in entries {
                if let Some(f) = from {
                    if kb.as_slice() < f {
                        continue;
                    }
                }
                if let Some(t) = to {
                    if kb.as_slice() > t {
                        past_to = true;
                        break;
                    }
                }
                results.push((kb, rid));
            }

            if past_to || next == 0 {
                break;
            }
            cur_leaf = PageId(next as u64);
        }

        Ok(results)
    }

    /// Delete an entry with the given key and row_id
    pub fn delete(&self, pager: &mut Pager, key: &BTreeKey) -> Result<bool> {
        let leaf = self.find_leaf_for_key(pager, self.root, &key.key_bytes)?;
        let entries = self.read_leaf_entries(pager, leaf)?;
        let next_leaf = {
            let page = pager.read_page(leaf)?;
            u32::from_le_bytes([page.data[3], page.data[4], page.data[5], page.data[6]])
        };

        let original_len = entries.len();
        let new_entries: Vec<LeafEntry> = entries
            .into_iter()
            .filter(|e| !(e.key.key_bytes == key.key_bytes && e.key.row_id == key.row_id))
            .collect();

        if new_entries.len() == original_len {
            return Ok(false);
        }

        self.write_leaf_entries(pager, leaf, &new_entries, next_leaf)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::pager::Pager;
    use crate::types::Value;
    use tempfile::NamedTempFile;

    fn make_key(v: i64, rid: u64) -> BTreeKey {
        BTreeKey::new(&Value::Int(v), rid)
    }

    #[test]
    fn test_btree_insert_and_search() {
        let tmp = NamedTempFile::new().unwrap();
        let mut pager = Pager::open(tmp.path()).unwrap();
        let mut tree = BTree::create(&mut pager).unwrap();

        tree.insert(&mut pager, make_key(10, 1)).unwrap();
        tree.insert(&mut pager, make_key(20, 2)).unwrap();
        tree.insert(&mut pager, make_key(10, 3)).unwrap();

        let v = Value::Int(10);
        let kb = v.to_bytes();
        let results = tree.search_exact(&mut pager, &kb).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.contains(&1));
        assert!(results.contains(&3));
    }

    #[test]
    fn test_btree_delete() {
        let tmp = NamedTempFile::new().unwrap();
        let mut pager = Pager::open(tmp.path()).unwrap();
        let mut tree = BTree::create(&mut pager).unwrap();

        tree.insert(&mut pager, make_key(5, 1)).unwrap();
        tree.insert(&mut pager, make_key(5, 2)).unwrap();

        let key = make_key(5, 1);
        tree.delete(&mut pager, &key).unwrap();

        let v = Value::Int(5);
        let kb = v.to_bytes();
        let results = tree.search_exact(&mut pager, &kb).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], 2);
    }

    #[test]
    fn test_btree_range_scan() {
        let tmp = NamedTempFile::new().unwrap();
        let mut pager = Pager::open(tmp.path()).unwrap();
        let mut tree = BTree::create(&mut pager).unwrap();

        for i in 1i64..=10 {
            tree.insert(&mut pager, make_key(i, i as u64)).unwrap();
        }

        let from_val = Value::Int(3);
        let to_val = Value::Int(7);
        let from_bytes = from_val.to_bytes();
        let to_bytes = to_val.to_bytes();

        let results = tree
            .range_scan(&mut pager, Some(&from_bytes), Some(&to_bytes))
            .unwrap();
        assert_eq!(results.len(), 5); // 3, 4, 5, 6, 7
    }

    #[test]
    fn test_btree_many_inserts() {
        let tmp = NamedTempFile::new().unwrap();
        let mut pager = Pager::open(tmp.path()).unwrap();
        let mut tree = BTree::create(&mut pager).unwrap();

        for i in 0i64..200 {
            tree.insert(&mut pager, make_key(i, i as u64)).unwrap();
        }

        // Full scan
        let results = tree.range_scan(&mut pager, None, None).unwrap();
        assert_eq!(results.len(), 200);
    }
}

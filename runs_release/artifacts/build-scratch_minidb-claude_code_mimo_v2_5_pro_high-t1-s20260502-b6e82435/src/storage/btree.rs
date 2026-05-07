use crate::error::{MiniDbError, Result};
use crate::storage::page::{Page, PageId, PAGE_SIZE};
use crate::types::Value;

/// B-tree secondary index for speeding up equality and range lookups.
///
/// Page layout:
///   [0]:     node_type (u8): 0 = leaf, 1 = internal
///   [1..3]:  key_count (u16)
///   [3..11]: next_leaf (u64): for leaf nodes, pointer to next leaf (0 if none)
///   [11..]:  entries
///
/// Leaf entry: [key_value serialized][row_id (u64)]
/// Internal entry: [key_value serialized][child_page_id (u64)]
/// Internal nodes have one extra child pointer at the end.
const BTREE_HEADER_SIZE: usize = 11;
const MAX_KEYS: usize = 50; // approximate, depends on key sizes

pub struct BTree {
    root_page: PageId,
}

impl BTree {
    pub fn new(root_page: PageId) -> Self {
        Self { root_page }
    }

    pub fn root_page(&self) -> PageId {
        self.root_page
    }

    pub fn init_leaf(page: &mut Page) {
        page.write_u8(0, 0); // leaf
        page.write_u16(1, 0); // key_count
        page.write_u64(3, 0); // next_leaf
    }

    pub fn init_internal(page: &mut Page) {
        page.write_u8(0, 1); // internal
        page.write_u16(1, 0); // key_count
        page.write_u64(3, 0); // unused for internal
    }

    fn is_leaf(page: &Page) -> bool {
        page.read_u8(0) == 0
    }

    fn key_count(page: &Page) -> u16 {
        page.read_u16(1)
    }

    fn set_key_count(page: &mut Page, count: u16) {
        page.write_u16(1, count);
    }

    /// Insert a (key, row_id) pair into the B-tree.
    /// This is a simplified in-place insert for leaf nodes only.
    pub fn insert(&self, page: &mut Page, key: &Value, row_id: u64) -> Result<()> {
        if !Self::is_leaf(page) {
            return Err(MiniDbError::Index(
                "cannot insert directly into internal node".into(),
            ));
        }

        let count = Self::key_count(page) as usize;
        let mut pos = BTREE_HEADER_SIZE;

        // Find insertion position (sorted by key)
        let mut insert_idx = count;
        for i in 0..count {
            let (existing_key, _) = Self::read_leaf_entry(page, pos)?;
            if Self::value_cmp(key, &existing_key) == std::cmp::Ordering::Less {
                insert_idx = i;
                break;
            }
            pos += Self::leaf_entry_size(&existing_key);
        }

        // Shift entries after insert position
        let serialized = Self::serialize_leaf_entry(key, row_id);
        let entry_size = serialized.len();

        // Find the byte offset for insert_idx
        let mut byte_offset = BTREE_HEADER_SIZE;
        for _i in 0..insert_idx {
            let (k, _) = Self::read_leaf_entry(page, byte_offset)?;
            byte_offset += Self::leaf_entry_size(&k);
        }

        // Shift existing data right
        let _total_data_start = BTREE_HEADER_SIZE;
        let shift_start = byte_offset;
        let data_end = {
            let mut p = BTREE_HEADER_SIZE;
            for _ in 0..count {
                let (k, _) = Self::read_leaf_entry(page, p)?;
                p += Self::leaf_entry_size(&k);
            }
            p
        };

        // Make room
        if data_end + entry_size > PAGE_SIZE {
            return Err(MiniDbError::Index("B-tree leaf page is full".into()));
        }

        // Shift bytes right
        let page_data = page.as_mut_slice();
        for i in (shift_start..data_end).rev() {
            page_data[i + entry_size] = page_data[i];
        }

        // Write new entry
        let mut write_pos = shift_start;
        for &byte in &serialized {
            page.as_mut_slice()[write_pos] = byte;
            write_pos += 1;
        }

        Self::set_key_count(page, count as u16 + 1);
        Ok(())
    }

    /// Search for exact key matches, returning all matching row_ids.
    pub fn search_eq(page: &Page, key: &Value) -> Result<Vec<u64>> {
        if !Self::is_leaf(page) {
            return Err(MiniDbError::Index("search_eq on internal node".into()));
        }

        let count = Self::key_count(page) as usize;
        let mut pos = BTREE_HEADER_SIZE;
        let mut results = Vec::new();

        for _ in 0..count {
            let (k, rid) = Self::read_leaf_entry(page, pos)?;
            let cmp = Self::value_cmp(&k, key);
            match cmp {
                std::cmp::Ordering::Equal => results.push(rid),
                std::cmp::Ordering::Greater => break, // past the range
                _ => {}
            }
            pos += Self::leaf_entry_size(&k);
        }

        Ok(results)
    }

    /// Search for range: key >= low AND key <= high
    pub fn search_range(page: &Page, low: &Value, high: &Value) -> Result<Vec<u64>> {
        if !Self::is_leaf(page) {
            return Err(MiniDbError::Index("search_range on internal node".into()));
        }

        let count = Self::key_count(page) as usize;
        let mut pos = BTREE_HEADER_SIZE;
        let mut results = Vec::new();

        for _ in 0..count {
            let (k, rid) = Self::read_leaf_entry(page, pos)?;
            let cmp_low = Self::value_cmp(&k, low);
            let cmp_high = Self::value_cmp(&k, high);

            if cmp_low != std::cmp::Ordering::Less && cmp_high != std::cmp::Ordering::Greater {
                results.push(rid);
            }

            if cmp_high == std::cmp::Ordering::Greater {
                break;
            }

            pos += Self::leaf_entry_size(&k);
        }

        Ok(results)
    }

    /// Delete a (key, row_id) entry from the B-tree leaf.
    pub fn delete(&self, page: &mut Page, key: &Value, row_id: u64) -> Result<bool> {
        if !Self::is_leaf(page) {
            return Err(MiniDbError::Index(
                "cannot delete from internal node".into(),
            ));
        }

        let count = Self::key_count(page) as usize;
        let mut pos = BTREE_HEADER_SIZE;

        for _i in 0..count {
            let (k, rid) = Self::read_leaf_entry(page, pos)?;
            let entry_size = Self::leaf_entry_size(&k);

            if Self::value_cmp(&k, key) == std::cmp::Ordering::Equal && rid == row_id {
                // Shift entries left
                let page_data = page.as_mut_slice();
                let src = pos + entry_size;
                let dst = pos;
                let remaining = {
                    let mut p = BTREE_HEADER_SIZE;
                    for _ in 0..count {
                        let (ek, _) = Self::read_leaf_entry_from_slice(&page_data[p..])?;
                        p += Self::leaf_entry_size(&ek);
                    }
                    p - src
                };
                page_data.copy_within(src..src + remaining, dst);
                Self::set_key_count(page, count as u16 - 1);
                return Ok(true);
            }

            pos += entry_size;
        }

        Ok(false)
    }

    fn value_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
    }

    fn serialize_leaf_entry(key: &Value, row_id: u64) -> Vec<u8> {
        let mut buf = Vec::new();
        key.serialize_to(&mut buf);
        buf.extend_from_slice(&row_id.to_le_bytes());
        buf
    }

    fn leaf_entry_size(key: &Value) -> usize {
        let key_size = match key {
            Value::Null => 1,
            Value::Int(_) => 9,
            Value::Real(_) => 9,
            Value::Text(s) => 5 + s.len(),
            Value::Bool(_) => 2,
        };
        key_size + 8 // + row_id
    }

    fn read_leaf_entry(page: &Page, offset: usize) -> Result<(Value, u64)> {
        let data = &page.as_slice()[offset..];
        Self::read_leaf_entry_from_slice(data)
    }

    fn read_leaf_entry_from_slice(data: &[u8]) -> Result<(Value, u64)> {
        let (key, key_consumed) = Value::deserialize_from(data)?;
        let rid_offset = key_consumed;
        if data.len() < rid_offset + 8 {
            return Err(MiniDbError::Index("truncated leaf entry".into()));
        }
        let row_id = u64::from_le_bytes(data[rid_offset..rid_offset + 8].try_into().unwrap());
        Ok((key, row_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_btree_insert_and_search() {
        let mut page = Page::new(PageId(0));
        BTree::init_leaf(&mut page);

        let btree = BTree::new(PageId(0));
        btree.insert(&mut page, &Value::Int(3), 30).unwrap();
        btree.insert(&mut page, &Value::Int(1), 10).unwrap();
        btree.insert(&mut page, &Value::Int(2), 20).unwrap();

        let results = BTree::search_eq(&page, &Value::Int(2)).unwrap();
        assert_eq!(results, vec![20]);

        let results = BTree::search_eq(&page, &Value::Int(5)).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_btree_range_search() {
        let mut page = Page::new(PageId(0));
        BTree::init_leaf(&mut page);

        let btree = BTree::new(PageId(0));
        for i in 1..=10 {
            btree
                .insert(&mut page, &Value::Int(i), i as u64 * 10)
                .unwrap();
        }

        let results = BTree::search_range(&page, &Value::Int(3), &Value::Int(7)).unwrap();
        assert_eq!(results, vec![30, 40, 50, 60, 70]);
    }

    #[test]
    fn test_btree_delete() {
        let mut page = Page::new(PageId(0));
        BTree::init_leaf(&mut page);

        let btree = BTree::new(PageId(0));
        btree.insert(&mut page, &Value::Int(1), 10).unwrap();
        btree.insert(&mut page, &Value::Int(2), 20).unwrap();
        btree.insert(&mut page, &Value::Int(3), 30).unwrap();

        assert!(btree.delete(&mut page, &Value::Int(2), 20).unwrap());

        let results = BTree::search_eq(&page, &Value::Int(2)).unwrap();
        assert!(results.is_empty());
        assert_eq!(BTree::key_count(&page), 2);
    }

    #[test]
    fn test_btree_sorted_order() {
        let mut page = Page::new(PageId(0));
        BTree::init_leaf(&mut page);

        let btree = BTree::new(PageId(0));
        btree.insert(&mut page, &Value::Int(5), 50).unwrap();
        btree.insert(&mut page, &Value::Int(1), 10).unwrap();
        btree.insert(&mut page, &Value::Int(3), 30).unwrap();
        btree.insert(&mut page, &Value::Int(2), 20).unwrap();
        btree.insert(&mut page, &Value::Int(4), 40).unwrap();

        let results = BTree::search_range(&page, &Value::Int(0), &Value::Int(100)).unwrap();
        assert_eq!(results, vec![10, 20, 30, 40, 50]);
    }
}

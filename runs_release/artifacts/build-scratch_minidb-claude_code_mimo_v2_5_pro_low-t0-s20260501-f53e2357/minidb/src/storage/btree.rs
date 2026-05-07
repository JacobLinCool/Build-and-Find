use crate::error::Result;
use crate::storage::page::{PageId, PageManager, PAGE_SIZE};
use crate::types::Value;

// B-tree page layout:
// [page_type:1][is_leaf:1][num_keys:2][parent:8][next_leaf:8]
// For leaf: entries = [(key_value:n)(row_page:8)(row_slot:4)]*
// For internal: entries = [(key_value:n)(child_page:8)]*, then rightmost child at end
//
// Key serialization: [type_tag:1][data...]
//   Null: 0
//   Int: 1 + 8 bytes
//   Real: 2 + 8 bytes (as bits)
//   Text: 3 + len:2 + bytes
//   Bool: 4 + 1 byte

const PAGE_TYPE_BTREE: u8 = 2;
const BTREE_HEADER: usize = 20; // type(1) + is_leaf(1) + num_keys(2) + parent(8) + next_leaf(8)

fn serialize_key(val: &Value) -> Vec<u8> {
    let mut buf = Vec::new();
    match val {
        Value::Null => buf.push(0),
        Value::Int(v) => {
            buf.push(1);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Value::Real(v) => {
            buf.push(2);
            buf.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        Value::Text(s) => {
            buf.push(3);
            let bytes = s.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        Value::Bool(b) => {
            buf.push(4);
            buf.push(if *b { 1 } else { 0 });
        }
    }
    buf
}

fn deserialize_key(data: &[u8]) -> (Value, usize) {
    let mut pos = 0;
    let val = match data[pos] {
        0 => {
            pos += 1;
            Value::Null
        }
        1 => {
            pos += 1;
            let v = i64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
            pos += 8;
            Value::Int(v)
        }
        2 => {
            pos += 1;
            let bits = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
            pos += 8;
            Value::Real(f64::from_bits(bits))
        }
        3 => {
            pos += 1;
            let len = u16::from_le_bytes(data[pos..pos + 2].try_into().unwrap()) as usize;
            pos += 2;
            let s = String::from_utf8_lossy(&data[pos..pos + len]).to_string();
            pos += len;
            Value::Text(s)
        }
        4 => {
            pos += 1;
            let b = data[pos] != 0;
            pos += 1;
            Value::Bool(b)
        }
        _ => {
            pos += 1;
            Value::Null
        }
    };
    (val, pos)
}

fn compare_keys(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Real(x), Value::Real(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Text(x), Value::Text(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Int(x), Value::Real(y)) => (*x as f64).partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Real(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(Ordering::Equal),
        _ => Ordering::Equal,
    }
}

pub struct BTreeIndex {
    pub index_id: u64,
    pub root_page: PageId,
}

#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub key: Value,
    pub row_page: PageId,
    pub row_slot: u32,
}

impl BTreeIndex {
    pub fn new(index_id: u64, root_page: PageId) -> Self {
        Self {
            index_id,
            root_page,
        }
    }

    pub fn init_root(page: &mut [u8; PAGE_SIZE]) {
        page[0] = PAGE_TYPE_BTREE;
        page[1] = 1; // is_leaf = true
        page[2..4].copy_from_slice(&0u16.to_le_bytes()); // num_keys
        page[4..12].copy_from_slice(&u64::MAX.to_le_bytes()); // parent
        page[12..20].copy_from_slice(&u64::MAX.to_le_bytes()); // next_leaf
    }

    pub fn insert(
        &self,
        pm: &mut PageManager,
        key: &Value,
        row_page: PageId,
        row_slot: u32,
    ) -> Result<()> {
        let root = pm.read_page(self.root_page)?;
        let is_leaf = root[1] == 1;

        if is_leaf {
            let mut page = root;
            Self::insert_into_leaf(&mut page, key, row_page, row_slot)?;
            pm.write_page(self.root_page, &page)?;
        } else {
            // Find the correct child
            let child = self.find_leaf_page(pm, key)?;
            let mut child_page = pm.read_page(child)?;
            Self::insert_into_leaf(&mut child_page, key, row_page, row_slot)?;
            pm.write_page(child, &child_page)?;
        }
        Ok(())
    }

    fn insert_into_leaf(
        page: &mut [u8; PAGE_SIZE],
        key: &Value,
        row_page: PageId,
        row_slot: u32,
    ) -> Result<()> {
        let num_keys = u16::from_le_bytes(page[2..4].try_into().unwrap()) as usize;
        let key_bytes = serialize_key(key);

        // Find insertion point
        let mut entries: Vec<(Value, PageId, u32)> = Vec::new();
        let mut pos = BTREE_HEADER;
        for _ in 0..num_keys {
            let (k, consumed) = deserialize_key(&page[pos..]);
            pos += consumed;
            let rp = PageId(u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap()));
            pos += 8;
            let rs = u32::from_le_bytes(page[pos..pos + 4].try_into().unwrap());
            pos += 4;
            entries.push((k, rp, rs));
        }

        // Insert at sorted position
        let insert_idx = entries
            .partition_point(|(k, _, _)| compare_keys(k, key) != std::cmp::Ordering::Greater);
        entries.insert(insert_idx, (key.clone(), row_page, row_slot));

        // Rebuild page
        let mut pos = BTREE_HEADER;
        for (k, rp, rs) in &entries {
            let kb = serialize_key(k);
            page[pos..pos + kb.len()].copy_from_slice(&kb);
            pos += kb.len();
            page[pos..pos + 8].copy_from_slice(&rp.0.to_le_bytes());
            pos += 8;
            page[pos..pos + 4].copy_from_slice(&rs.to_le_bytes());
            pos += 4;
        }
        page[2..4].copy_from_slice(&(entries.len() as u16).to_le_bytes());
        Ok(())
    }

    fn find_leaf_page(&self, pm: &mut PageManager, key: &Value) -> Result<PageId> {
        let mut current = self.root_page;
        loop {
            let page = pm.read_page(current)?;
            let is_leaf = page[1] == 1;
            if is_leaf {
                return Ok(current);
            }

            // Internal node: parse entries
            let num_keys = u16::from_le_bytes(page[2..4].try_into().unwrap()) as usize;
            let mut pos = BTREE_HEADER;
            let mut found_child = None;

            for i in 0..num_keys {
                let (k, consumed) = deserialize_key(&page[pos..]);
                pos += consumed;
                let child = PageId(u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap()));
                pos += 8;

                if compare_keys(key, &k) == std::cmp::Ordering::Less {
                    found_child = Some(child);
                    break;
                }
            }

            if found_child.is_none() {
                // Rightmost child
                let rightmost =
                    u64::from_le_bytes(page[PAGE_SIZE - 8..PAGE_SIZE].try_into().unwrap());
                found_child = Some(PageId(rightmost));
            }

            current = found_child.unwrap();
        }
    }

    pub fn search_equal(&self, pm: &mut PageManager, key: &Value) -> Result<Vec<IndexEntry>> {
        let leaf = self.find_leaf_page(pm, key)?;
        let page = pm.read_page(leaf)?;
        let num_keys = u16::from_le_bytes(page[2..4].try_into().unwrap()) as usize;

        let mut results = Vec::new();
        let mut pos = BTREE_HEADER;
        for _ in 0..num_keys {
            let (k, consumed) = deserialize_key(&page[pos..]);
            pos += consumed;
            let rp = PageId(u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap()));
            pos += 8;
            let rs = u32::from_le_bytes(page[pos..pos + 4].try_into().unwrap());
            pos += 4;

            if compare_keys(&k, key) == std::cmp::Ordering::Equal {
                results.push(IndexEntry {
                    key: k,
                    row_page: rp,
                    row_slot: rs,
                });
            }
        }
        Ok(results)
    }

    pub fn search_range(
        &self,
        pm: &mut PageManager,
        low: Option<&Value>,
        high: Option<&Value>,
    ) -> Result<Vec<IndexEntry>> {
        let start_key = low.cloned().unwrap_or(Value::Null);
        let leaf = self.find_leaf_page(pm, &start_key)?;

        let mut results = Vec::new();
        let mut current = leaf;

        loop {
            let page = pm.read_page(current)?;
            let num_keys = u16::from_le_bytes(page[2..4].try_into().unwrap()) as usize;
            let next = u64::from_le_bytes(page[12..20].try_into().unwrap());

            let mut pos = BTREE_HEADER;
            for _ in 0..num_keys {
                let (k, consumed) = deserialize_key(&page[pos..]);
                pos += consumed;
                let rp = PageId(u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap()));
                pos += 8;
                let rs = u32::from_le_bytes(page[pos..pos + 4].try_into().unwrap());
                pos += 4;

                let past_low = match low {
                    Some(lo) => compare_keys(&k, lo) != std::cmp::Ordering::Less,
                    None => true,
                };
                let past_high = match high {
                    Some(hi) => compare_keys(&k, hi) != std::cmp::Ordering::Less,
                    None => false,
                };

                if past_low && !past_high {
                    results.push(IndexEntry {
                        key: k,
                        row_page: rp,
                        row_slot: rs,
                    });
                }

                if past_high {
                    return Ok(results);
                }
            }

            if next == u64::MAX {
                break;
            }
            current = PageId(next);
        }

        Ok(results)
    }

    pub fn delete(
        &self,
        pm: &mut PageManager,
        key: &Value,
        row_page: PageId,
        row_slot: u32,
    ) -> Result<()> {
        let leaf = self.find_leaf_page(pm, key)?;
        let mut page = pm.read_page(leaf)?;
        let num_keys = u16::from_le_bytes(page[2..4].try_into().unwrap()) as usize;

        let mut entries: Vec<(Value, PageId, u32)> = Vec::new();
        let mut pos = BTREE_HEADER;
        for _ in 0..num_keys {
            let (k, consumed) = deserialize_key(&page[pos..]);
            pos += consumed;
            let rp = PageId(u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap()));
            pos += 8;
            let rs = u32::from_le_bytes(page[pos..pos + 4].try_into().unwrap());
            pos += 4;
            entries.push((k, rp, rs));
        }

        entries.retain(|(k, rp, rs)| {
            !(compare_keys(k, key) == std::cmp::Ordering::Equal
                && *rp == row_page
                && *rs == row_slot)
        });

        // Rebuild page
        page[BTREE_HEADER..].fill(0);
        let mut pos = BTREE_HEADER;
        for (k, rp, rs) in &entries {
            let kb = serialize_key(k);
            page[pos..pos + kb.len()].copy_from_slice(&kb);
            pos += kb.len();
            page[pos..pos + 8].copy_from_slice(&rp.0.to_le_bytes());
            pos += 8;
            page[pos..pos + 4].copy_from_slice(&rs.to_le_bytes());
            pos += 4;
        }
        page[2..4].copy_from_slice(&(entries.len() as u16).to_le_bytes());

        pm.write_page(leaf, &page)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup() -> (PageManager, BTreeIndex) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::open(&path).unwrap();
        let pid = pm.allocate_page().unwrap();
        let mut page = [0u8; PAGE_SIZE];
        BTreeIndex::init_root(&mut page);
        pm.write_page(pid, &page).unwrap();
        (pm, BTreeIndex::new(1, pid))
    }

    #[test]
    fn test_insert_and_search() {
        let (mut pm, bt) = setup();

        bt.insert(&mut pm, &Value::Int(10), PageId(100), 0).unwrap();
        bt.insert(&mut pm, &Value::Int(5), PageId(100), 1).unwrap();
        bt.insert(&mut pm, &Value::Int(20), PageId(100), 2).unwrap();

        let results = bt.search_equal(&mut pm, &Value::Int(10)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].row_slot, 0);

        let results = bt.search_equal(&mut pm, &Value::Int(99)).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_range_search() {
        let (mut pm, bt) = setup();

        for i in 0..10 {
            bt.insert(&mut pm, &Value::Int(i), PageId(100), i as u32)
                .unwrap();
        }

        let results = bt
            .search_range(&mut pm, Some(&Value::Int(3)), Some(&Value::Int(7)))
            .unwrap();
        assert_eq!(results.len(), 4); // 3, 4, 5, 6
    }

    #[test]
    fn test_delete() {
        let (mut pm, bt) = setup();

        bt.insert(&mut pm, &Value::Int(1), PageId(100), 0).unwrap();
        bt.insert(&mut pm, &Value::Int(2), PageId(100), 1).unwrap();

        bt.delete(&mut pm, &Value::Int(1), PageId(100), 0).unwrap();

        let results = bt.search_equal(&mut pm, &Value::Int(1)).unwrap();
        assert!(results.is_empty());

        let results = bt.search_equal(&mut pm, &Value::Int(2)).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_text_keys() {
        let (mut pm, bt) = setup();

        bt.insert(&mut pm, &Value::Text("banana".into()), PageId(1), 0)
            .unwrap();
        bt.insert(&mut pm, &Value::Text("apple".into()), PageId(1), 1)
            .unwrap();
        bt.insert(&mut pm, &Value::Text("cherry".into()), PageId(1), 2)
            .unwrap();

        let results = bt
            .search_range(
                &mut pm,
                Some(&Value::Text("b".into())),
                Some(&Value::Text("c".into())),
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, Value::Text("banana".into()));
    }
}

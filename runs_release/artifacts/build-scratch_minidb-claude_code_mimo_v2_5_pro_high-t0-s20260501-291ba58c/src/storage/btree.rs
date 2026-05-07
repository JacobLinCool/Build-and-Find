use crate::error::{MiniDbError, Result};
use crate::storage::page::{PageId, PageManager, PAGE_SIZE};
use crate::types::Value;
use std::io::Write;

/// B-tree secondary index on (index_key, row_id).
/// Node page layout:
///   [1 byte: is_leaf] [2 bytes: count] [entries...]
/// Leaf entry: [serialized key] [8 bytes: row_id (page_id u32 | slot_id u32)]
/// Internal entry: [serialized key] [8 bytes: child_page_id]
/// The last internal entry has no key (sentinel), just child_page_id.
///
/// row_id = (page_id << 32) | slot_id

const NODE_HEADER: usize = 3;

#[derive(Debug, Clone)]
pub struct RowId {
    pub page_id: u32,
    pub slot_id: u32,
}

impl RowId {
    pub fn to_u64(&self) -> u64 {
        ((self.page_id as u64) << 32) | (self.slot_id as u64)
    }

    pub fn from_u64(v: u64) -> Self {
        Self {
            page_id: (v >> 32) as u32,
            slot_id: v as u32,
        }
    }
}

fn serialize_key(key: &Value) -> Vec<u8> {
    let mut buf = Vec::new();
    match key {
        Value::Null => buf.push(0),
        Value::Int(i) => {
            buf.push(1);
            buf.extend_from_slice(&i.to_le_bytes());
        }
        Value::Real(r) => {
            buf.push(2);
            buf.extend_from_slice(&r.to_le_bytes());
        }
        Value::Text(s) => {
            buf.push(3);
            let bytes = s.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
            buf.write_all(bytes).unwrap();
        }
        Value::Bool(b) => {
            buf.push(4);
            buf.push(if *b { 1 } else { 0 });
        }
    }
    buf
}

fn deserialize_key(data: &[u8], pos: &mut usize) -> Result<Value> {
    if *pos >= data.len() {
        return Err(MiniDbError::BTree("unexpected end of key data".to_string()));
    }
    let tag = data[*pos];
    *pos += 1;
    match tag {
        0 => Ok(Value::Null),
        1 => {
            if *pos + 8 > data.len() {
                return Err(MiniDbError::BTree("truncated Int key".to_string()));
            }
            let i = i64::from_le_bytes(data[*pos..*pos + 8].try_into().unwrap());
            *pos += 8;
            Ok(Value::Int(i))
        }
        2 => {
            if *pos + 8 > data.len() {
                return Err(MiniDbError::BTree("truncated Real key".to_string()));
            }
            let r = f64::from_le_bytes(data[*pos..*pos + 8].try_into().unwrap());
            *pos += 8;
            Ok(Value::Real(r))
        }
        3 => {
            if *pos + 2 > data.len() {
                return Err(MiniDbError::BTree("truncated Text key len".to_string()));
            }
            let slen = u16::from_le_bytes(data[*pos..*pos + 2].try_into().unwrap()) as usize;
            *pos += 2;
            if *pos + slen > data.len() {
                return Err(MiniDbError::BTree("truncated Text key".to_string()));
            }
            let s = String::from_utf8_lossy(&data[*pos..*pos + slen]).to_string();
            *pos += slen;
            Ok(Value::Text(s))
        }
        4 => {
            if *pos >= data.len() {
                return Err(MiniDbError::BTree("truncated Bool key".to_string()));
            }
            let b = data[*pos] != 0;
            *pos += 1;
            Ok(Value::Bool(b))
        }
        _ => Err(MiniDbError::BTree(format!("unknown key tag: {tag}"))),
    }
}

fn key_size(key: &Value) -> usize {
    match key {
        Value::Null => 1,
        Value::Int(_) => 1 + 8,
        Value::Real(_) => 1 + 8,
        Value::Text(s) => 1 + 2 + s.len(),
        Value::Bool(_) => 1 + 1,
    }
}

use std::cell::Cell;

pub struct BTreeIndex {
    pub name: String,
    root_page: Cell<PageId>,
}

impl BTreeIndex {
    pub fn new(name: &str, root_page: PageId) -> Self {
        Self {
            name: name.to_string(),
            root_page: Cell::new(root_page),
        }
    }

    pub fn root_page(&self) -> PageId {
        self.root_page.get()
    }

    pub fn create(pm: &mut PageManager, name: &str) -> Result<Self> {
        let pid = pm.alloc_page()?;
        let mut page = [0u8; PAGE_SIZE];
        page[0] = 1; // is_leaf = true
        page[1..3].copy_from_slice(&0u16.to_le_bytes()); // count = 0
        pm.write_page(pid, &page)?;
        Ok(Self::new(name, pid))
    }

    pub fn insert(&self, pm: &mut PageManager, key: &Value, row_id: &RowId) -> Result<()> {
        let split = self.insert_internal(pm, self.root_page.get(), key, row_id)?;
        if let Some((split_key, new_page)) = split {
            // Root split: create new root
            let new_root = pm.alloc_page()?;
            let mut page = [0u8; PAGE_SIZE];
            page[0] = 0; // internal
                         // 3 entries: left child, (split_key, right child), sentinel
            page[1..3].copy_from_slice(&2u16.to_le_bytes());
            let mut pos = NODE_HEADER;
            // First entry: just child pointer (sentinel-style for left)
            let sk = serialize_key(&split_key);
            page[pos] = sk.len() as u8;
            pos += 1;
            page[pos..pos + sk.len()].copy_from_slice(&sk);
            pos += sk.len();
            page[pos..pos + 8].copy_from_slice(&new_page.0.to_le_bytes());
            pos += 8;
            // Sentinel: zero-length key, child = old root
            page[pos] = 0;
            pos += 1;
            page[pos..pos + 8].copy_from_slice(&self.root_page.get().0.to_le_bytes());
            pm.write_page(new_root, &page)?;
            self.root_page.set(new_root);
            log::info!("B-tree root split: new root at {:?}", new_root);
        }
        Ok(())
    }

    fn insert_internal(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        key: &Value,
        row_id: &RowId,
    ) -> Result<Option<(Value, PageId)>> {
        let mut page = pm.read_page(page_id)?;
        let is_leaf = page[0] != 0;
        let count = u16::from_le_bytes(page[1..3].try_into().unwrap());

        if is_leaf {
            return self.insert_leaf(pm, page_id, &mut *page, count, key, row_id);
        }

        // Find child to insert into
        let mut pos = NODE_HEADER;
        let mut child_idx = 0;
        let mut children = Vec::new();
        let mut keys = Vec::new();

        for _i in 0..count {
            let klen = page[pos] as usize;
            pos += 1;
            if klen > 0 {
                let k = {
                    let mut p = 0;
                    deserialize_key(&page[pos..pos + klen], &mut p)?
                };
                keys.push(k);
                pos += klen;
            }
            let child_id = PageId(u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap()));
            children.push(child_id);
            pos += 8;
        }

        // Find which child to recurse into
        for (i, k) in keys.iter().enumerate() {
            if key.compare_nulls_last(k) != std::cmp::Ordering::Greater {
                child_idx = i;
                break;
            }
            child_idx = i + 1;
        }

        let split = self.insert_internal(pm, children[child_idx], key, row_id)?;
        if let Some((split_key, new_page)) = split {
            // Insert split_key and new_page into this node
            self.insert_internal_node(pm, page_id, &split_key, new_page)
        } else {
            Ok(None)
        }
    }

    fn insert_internal_node(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        key: &Value,
        new_child: PageId,
    ) -> Result<Option<(Value, PageId)>> {
        let mut page = pm.read_page(page_id)?;
        let count = u16::from_le_bytes(page[1..3].try_into().unwrap());
        let sk = serialize_key(key);
        let entry_size = 1 + sk.len() + 8;

        // Calculate current size
        let mut current_size = NODE_HEADER;
        let mut pos = NODE_HEADER;
        for _ in 0..count {
            let klen = page[pos] as usize;
            current_size += 1 + klen + 8;
            pos += 1 + klen + 8;
        }

        if current_size + entry_size + 9 > PAGE_SIZE {
            // Need to split this node
            return self.split_internal(pm, page_id, key, new_child);
        }

        // Find insertion point
        let mut insert_pos = NODE_HEADER;
        let mut prev_pos = NODE_HEADER;
        for _ in 0..count {
            let klen = page[insert_pos] as usize;
            if klen > 0 {
                let existing_key = {
                    let mut p = 0;
                    deserialize_key(&page[insert_pos + 1..insert_pos + 1 + klen], &mut p)?
                };
                if key.compare_nulls_last(&existing_key) != std::cmp::Ordering::Greater {
                    break;
                }
            }
            prev_pos = insert_pos;
            insert_pos += 1 + klen + 8;
        }

        // Shift data after insert point
        let shift = entry_size;
        let data_end = current_size;
        for i in (insert_pos..data_end).rev() {
            if i + shift < PAGE_SIZE {
                page[i + shift] = page[i];
            }
        }

        // Write new entry
        let mut p = insert_pos;
        page[p] = sk.len() as u8;
        p += 1;
        page[p..p + sk.len()].copy_from_slice(&sk);
        p += sk.len();
        page[p..p + 8].copy_from_slice(&new_child.0.to_le_bytes());

        let new_count = count + 1;
        page[1..3].copy_from_slice(&new_count.to_le_bytes());
        pm.write_page(page_id, &page)?;
        Ok(None)
    }

    fn split_internal(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        new_key: &Value,
        new_child: PageId,
    ) -> Result<Option<(Value, PageId)>> {
        // Collect all entries plus the new one
        let page = pm.read_page(page_id)?;
        let count = u16::from_le_bytes(page[1..3].try_into().unwrap());
        let is_leaf = page[0] != 0;

        let mut all_keys = Vec::new();
        let mut all_children = Vec::new();
        let mut pos = NODE_HEADER;

        for _ in 0..count {
            let klen = page[pos] as usize;
            pos += 1;
            let child = PageId(u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap()));
            if klen > 0 {
                let k = {
                    let mut p = 0;
                    deserialize_key(&page[pos..pos + klen], &mut p)?
                };
                all_keys.push(Some(k));
            } else {
                all_keys.push(None);
            }
            all_children.push(child);
            pos += 8;
        }

        // Insert new entry at correct position
        let mut insert_idx = all_keys.len();
        for (i, k) in all_keys.iter().enumerate() {
            if let Some(k) = k {
                if new_key.compare_nulls_last(k) != std::cmp::Ordering::Greater {
                    insert_idx = i;
                    break;
                }
            }
        }
        all_keys.insert(insert_idx, Some(new_key.clone()));
        all_children.insert(insert_idx + 1, new_child);

        // Split: left half stays, right half goes to new page
        let mid = all_keys.len() / 2;
        let mid_key = all_keys[mid].clone().unwrap_or(Value::Null);

        // Rebuild left page
        let mut left = [0u8; PAGE_SIZE];
        left[0] = if is_leaf { 1 } else { 0 };
        let mut lp = NODE_HEADER;
        let left_count = mid as u16;
        for i in 0..mid {
            if let Some(k) = &all_keys[i] {
                let sk = serialize_key(k);
                left[lp] = sk.len() as u8;
                lp += 1;
                left[lp..lp + sk.len()].copy_from_slice(&sk);
                lp += sk.len();
            } else {
                left[lp] = 0;
                lp += 1;
            }
            left[lp..lp + 8].copy_from_slice(&all_children[i].0.to_le_bytes());
            lp += 8;
        }
        left[1..3].copy_from_slice(&left_count.to_le_bytes());
        pm.write_page(page_id, &left)?;

        // Create right page
        let right_pid = pm.alloc_page()?;
        let mut right = [0u8; PAGE_SIZE];
        right[0] = if is_leaf { 1 } else { 0 };
        let mut rp = NODE_HEADER;
        let right_count = (all_keys.len() - mid - 1) as u16;
        for i in (mid + 1)..all_keys.len() {
            if let Some(k) = &all_keys[i] {
                let sk = serialize_key(k);
                right[rp] = sk.len() as u8;
                rp += 1;
                right[rp..rp + sk.len()].copy_from_slice(&sk);
                rp += sk.len();
            } else {
                right[rp] = 0;
                rp += 1;
            }
            right[rp..rp + 8].copy_from_slice(&all_children[i].0.to_le_bytes());
            rp += 8;
        }
        right[1..3].copy_from_slice(&right_count.to_le_bytes());
        pm.write_page(right_pid, &right)?;

        Ok(Some((mid_key, right_pid)))
    }

    fn insert_leaf(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        page: &mut [u8; PAGE_SIZE],
        count: u16,
        key: &Value,
        row_id: &RowId,
    ) -> Result<Option<(Value, PageId)>> {
        let sk = serialize_key(key);
        let entry_size = 1 + sk.len() + 8;

        // Calculate current data size
        let mut current_size = NODE_HEADER;
        let mut pos = NODE_HEADER;
        for _ in 0..count {
            let klen = page[pos] as usize;
            current_size += 1 + klen + 8;
            pos += 1 + klen + 8;
        }

        if current_size + entry_size + 9 > PAGE_SIZE {
            // Need to split
            return self.split_leaf(pm, page_id, key, row_id);
        }

        // Find insertion point (sorted)
        let mut insert_pos = NODE_HEADER;
        for _ in 0..count {
            let klen = page[insert_pos] as usize;
            if klen > 0 {
                let existing = {
                    let mut p = 0;
                    deserialize_key(&page[insert_pos + 1..insert_pos + 1 + klen], &mut p)?
                };
                if key.compare_nulls_last(&existing) == std::cmp::Ordering::Less {
                    break;
                }
            }
            insert_pos += 1 + klen + 8;
        }

        // Shift
        let shift = entry_size;
        let data_end = current_size;
        for i in (insert_pos..data_end).rev() {
            if i + shift < PAGE_SIZE {
                page[i + shift] = page[i];
            }
        }

        // Write entry
        let mut p = insert_pos;
        page[p] = sk.len() as u8;
        p += 1;
        page[p..p + sk.len()].copy_from_slice(&sk);
        p += sk.len();
        let rid = row_id.to_u64();
        page[p..p + 8].copy_from_slice(&rid.to_le_bytes());

        let new_count = count + 1;
        page[1..3].copy_from_slice(&new_count.to_le_bytes());
        pm.write_page(page_id, page)?;
        Ok(None)
    }

    fn split_leaf(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        new_key: &Value,
        row_id: &RowId,
    ) -> Result<Option<(Value, PageId)>> {
        let page = pm.read_page(page_id)?;
        let count = u16::from_le_bytes(page[1..3].try_into().unwrap());

        // Collect all entries
        let mut entries: Vec<(Value, u64)> = Vec::new();
        let mut pos = NODE_HEADER;
        for _ in 0..count {
            let klen = page[pos] as usize;
            pos += 1;
            let mut k = Value::Null;
            if klen > 0 {
                let mut p = 0;
                k = deserialize_key(&page[pos..pos + klen], &mut p)?;
                pos += klen;
            }
            let rid = u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap());
            entries.push((k, rid));
            pos += 8;
        }

        // Insert new entry
        let new_rid = row_id.to_u64();
        let mut insert_idx = entries.len();
        for (i, (k, _)) in entries.iter().enumerate() {
            if new_key.compare_nulls_last(k) == std::cmp::Ordering::Less {
                insert_idx = i;
                break;
            }
        }
        entries.insert(insert_idx, (new_key.clone(), new_rid));

        let mid = entries.len() / 2;
        let mid_key = entries[mid].0.clone();

        // Rebuild left
        let mut left = [0u8; PAGE_SIZE];
        left[0] = 1;
        let mut lp = NODE_HEADER;
        for (k, rid) in entries.iter().take(mid) {
            let sk = serialize_key(k);
            left[lp] = sk.len() as u8;
            lp += 1;
            left[lp..lp + sk.len()].copy_from_slice(&sk);
            lp += sk.len();
            left[lp..lp + 8].copy_from_slice(&rid.to_le_bytes());
            lp += 8;
        }
        left[1..3].copy_from_slice(&(mid as u16).to_le_bytes());
        pm.write_page(page_id, &left)?;

        // Create right
        let right_pid = pm.alloc_page()?;
        let mut right = [0u8; PAGE_SIZE];
        right[0] = 1;
        let mut rp = NODE_HEADER;
        let right_count = entries.len() - mid;
        for (k, rid) in entries.iter().skip(mid) {
            let sk = serialize_key(k);
            right[rp] = sk.len() as u8;
            rp += 1;
            right[rp..rp + sk.len()].copy_from_slice(&sk);
            rp += sk.len();
            right[rp..rp + 8].copy_from_slice(&rid.to_le_bytes());
            rp += 8;
        }
        right[1..3].copy_from_slice(&(right_count as u16).to_le_bytes());
        pm.write_page(right_pid, &right)?;

        Ok(Some((mid_key, right_pid)))
    }

    pub fn search(&self, pm: &mut PageManager, key: &Value) -> Result<Vec<RowId>> {
        self.search_node(pm, self.root_page.get(), key)
    }

    fn search_node(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        key: &Value,
    ) -> Result<Vec<RowId>> {
        let page = pm.read_page(page_id)?;
        let is_leaf = page[0] != 0;
        let count = u16::from_le_bytes(page[1..3].try_into().unwrap());

        if is_leaf {
            let mut results = Vec::new();
            let mut pos = NODE_HEADER;
            for _ in 0..count {
                let klen = page[pos] as usize;
                pos += 1;
                let mut k = Value::Null;
                if klen > 0 {
                    let mut p = 0;
                    k = deserialize_key(&page[pos..pos + klen], &mut p)?;
                    pos += klen;
                }
                let rid = u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap());
                if k == *key {
                    results.push(RowId::from_u64(rid));
                }
                pos += 8;
            }
            return Ok(results);
        }

        // Internal node: find child
        let mut pos = NODE_HEADER;
        let mut last_child = None;
        for _ in 0..count {
            let klen = page[pos] as usize;
            pos += 1;
            if klen > 0 {
                let mut p = 0;
                let k = deserialize_key(&page[pos..pos + klen], &mut p)?;
                pos += klen;
                let child = PageId(u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap()));
                pos += 8;
                if key.compare_nulls_last(&k) != std::cmp::Ordering::Greater {
                    return self.search_node(pm, child, key);
                }
                last_child = Some(child);
            } else {
                let child = PageId(u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap()));
                pos += 8;
                last_child = Some(child);
            }
        }

        if let Some(child) = last_child {
            self.search_node(pm, child, key)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn range_search(
        &self,
        pm: &mut PageManager,
        low: Option<&Value>,
        high: Option<&Value>,
        include_low: bool,
        include_high: bool,
    ) -> Result<Vec<RowId>> {
        self.range_node(
            pm,
            self.root_page.get(),
            low,
            high,
            include_low,
            include_high,
        )
    }

    fn range_node(
        &self,
        pm: &mut PageManager,
        page_id: PageId,
        low: Option<&Value>,
        high: Option<&Value>,
        include_low: bool,
        include_high: bool,
    ) -> Result<Vec<RowId>> {
        let page = pm.read_page(page_id)?;
        let is_leaf = page[0] != 0;
        let count = u16::from_le_bytes(page[1..3].try_into().unwrap());

        if is_leaf {
            let mut results = Vec::new();
            let mut pos = NODE_HEADER;
            for _ in 0..count {
                let klen = page[pos] as usize;
                pos += 1;
                let mut k = Value::Null;
                if klen > 0 {
                    let mut p = 0;
                    k = deserialize_key(&page[pos..pos + klen], &mut p)?;
                    pos += klen;
                }
                let rid = u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap());
                let in_range = match (low, high) {
                    (Some(lo), Some(hi)) => {
                        let cmp_lo = k.compare_nulls_last(lo);
                        let cmp_hi = k.compare_nulls_last(hi);
                        (include_low && cmp_lo != std::cmp::Ordering::Less
                            || !include_low && cmp_lo == std::cmp::Ordering::Greater)
                            && (include_high && cmp_hi != std::cmp::Ordering::Greater
                                || !include_high && cmp_hi == std::cmp::Ordering::Less)
                    }
                    (Some(lo), None) => {
                        let cmp = k.compare_nulls_last(lo);
                        include_low && cmp != std::cmp::Ordering::Less
                            || !include_low && cmp == std::cmp::Ordering::Greater
                    }
                    (None, Some(hi)) => {
                        let cmp = k.compare_nulls_last(hi);
                        include_high && cmp != std::cmp::Ordering::Greater
                            || !include_low && cmp == std::cmp::Ordering::Less
                    }
                    (None, None) => true,
                };
                if in_range {
                    results.push(RowId::from_u64(rid));
                }
                pos += 8;
            }
            return Ok(results);
        }

        // Internal: recurse into relevant children
        let mut results = Vec::new();
        let mut pos = NODE_HEADER;
        let mut children = Vec::new();
        let mut keys = Vec::new();

        for _ in 0..count {
            let klen = page[pos] as usize;
            pos += 1;
            if klen > 0 {
                let mut p = 0;
                let k = deserialize_key(&page[pos..pos + klen], &mut p)?;
                pos += klen;
                keys.push(Some(k));
            } else {
                keys.push(None);
            }
            let child = PageId(u64::from_le_bytes(page[pos..pos + 8].try_into().unwrap()));
            children.push(child);
            pos += 8;
        }

        for (i, child) in children.iter().enumerate() {
            let should_visit = if i < keys.len() {
                if let Some(k) = &keys[i] {
                    let below_high = match high {
                        Some(hi) => k.compare_nulls_last(hi) != std::cmp::Ordering::Greater,
                        None => true,
                    };
                    let above_low = match low {
                        Some(lo) => k.compare_nulls_last(lo) == std::cmp::Ordering::Greater,
                        None => true,
                    };
                    below_high || above_low
                } else {
                    true
                }
            } else {
                true
            };
            if should_visit {
                let mut child_results =
                    self.range_node(pm, *child, low, high, include_low, include_high)?;
                results.append(&mut child_results);
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_btree_insert_search() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::create(&path).unwrap();
        let idx = BTreeIndex::create(&mut pm, "test_idx").unwrap();

        for i in 0..100 {
            let rid = RowId {
                page_id: 0,
                slot_id: i,
            };
            idx.insert(&mut pm, &Value::Int(i as i64), &rid).unwrap();
        }

        for i in 0..100 {
            let results = idx.search(&mut pm, &Value::Int(i as i64)).unwrap();
            assert!(!results.is_empty(), "key {i} not found");
            assert_eq!(results[0].slot_id, i as u32);
        }

        let not_found = idx.search(&mut pm, &Value::Int(999)).unwrap();
        assert!(not_found.is_empty());
    }

    #[test]
    fn test_btree_range_search() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::create(&path).unwrap();
        let idx = BTreeIndex::create(&mut pm, "test_idx").unwrap();

        for i in 0..50 {
            let rid = RowId {
                page_id: 0,
                slot_id: i,
            };
            idx.insert(&mut pm, &Value::Int(i as i64), &rid).unwrap();
        }

        let results = idx
            .range_search(
                &mut pm,
                Some(&Value::Int(10)),
                Some(&Value::Int(20)),
                true,
                true,
            )
            .unwrap();
        // Should find keys 10..=20
        for r in &results {
            assert!(r.slot_id >= 10 && r.slot_id <= 20);
        }
        assert!(!results.is_empty());
    }
}

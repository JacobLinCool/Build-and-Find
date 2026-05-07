use crate::types::{RowId, Value};
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub key: Value,
    pub row_id: RowId,
}

impl PartialEq for IndexEntry {
    fn eq(&self, other: &Self) -> bool {
        self.key.sql_eq(&other.key).unwrap_or(false) && self.row_id == other.row_id
    }
}

impl Eq for IndexEntry {}

impl PartialOrd for IndexEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for IndexEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanOp {
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
}

pub struct BTreeIndex {
    name: String,
    table: String,
    column: String,
    entries: BTreeMap<Value, Vec<RowId>>,
}

impl BTreeIndex {
    pub fn new(name: String, table: String, column: String) -> Self {
        BTreeIndex {
            name,
            table,
            column,
            entries: BTreeMap::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn table(&self) -> &str {
        &self.table
    }

    pub fn column(&self) -> &str {
        &self.column
    }

    pub fn insert(&mut self, key: Value, row_id: RowId) {
        log::debug!("BTree insert key={}, row_id={:?}", key, row_id);
        self.entries.entry(key).or_default().push(row_id);
    }

    pub fn remove_by_row_id(&mut self, row_id: &RowId) {
        for rids in self.entries.values_mut() {
            rids.retain(|r| r != row_id);
        }
        self.entries.retain(|_, rids| !rids.is_empty());
    }

    pub fn update_key(&mut self, _old_key: &Value, new_key: &Value, row_id: RowId) {
        self.remove_by_row_id(&row_id);
        self.insert(new_key.clone(), row_id);
    }

    pub fn lookup_eq(&self, key: &Value) -> Vec<RowId> {
        self.entries.get(key).cloned().unwrap_or_default()
    }

    pub fn scan(&self, op: ScanOp, key: &Value) -> Vec<RowId> {
        let mut result = Vec::new();
        match op {
            ScanOp::Eq => {
                if let Some(rids) = self.entries.get(key) {
                    result.extend(rids.iter().cloned());
                }
            }
            ScanOp::Gt => {
                for (k, rids) in self.entries.range((
                    std::ops::Bound::Excluded(key.clone()),
                    std::ops::Bound::Unbounded,
                )) {
                    if !k.is_null() && k.sql_gt(key).unwrap_or(false) {
                        result.extend(rids.iter().cloned());
                    }
                }
            }
            ScanOp::Ge => {
                for (k, rids) in self.entries.range((
                    std::ops::Bound::Included(key.clone()),
                    std::ops::Bound::Unbounded,
                )) {
                    if !k.is_null() && k.sql_ge(key).unwrap_or(false) {
                        result.extend(rids.iter().cloned());
                    }
                }
            }
            ScanOp::Lt => {
                for (k, rids) in self.entries.range((
                    std::ops::Bound::Unbounded,
                    std::ops::Bound::Excluded(key.clone()),
                )) {
                    if !k.is_null() && k.sql_lt(key).unwrap_or(false) {
                        result.extend(rids.iter().cloned());
                    }
                }
            }
            ScanOp::Le => {
                for (k, rids) in self.entries.range((
                    std::ops::Bound::Unbounded,
                    std::ops::Bound::Included(key.clone()),
                )) {
                    if !k.is_null() && k.sql_le(key).unwrap_or(false) {
                        result.extend(rids.iter().cloned());
                    }
                }
            }
        }
        result
    }

    pub fn all_entries(&self) -> Vec<(&Value, &Vec<RowId>)> {
        self.entries.iter().collect()
    }

    pub fn count(&self) -> usize {
        self.entries.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_btree_insert_and_lookup() {
        let mut idx = BTreeIndex::new("idx1".into(), "users".into(), "id".into());
        idx.insert(Value::Int(1), RowId(0));
        idx.insert(Value::Int(2), RowId(1));
        idx.insert(Value::Int(3), RowId(2));

        let result = idx.lookup_eq(&Value::Int(2));
        assert_eq!(result, vec![RowId(1)]);
    }

    #[test]
    fn test_btree_range_scan() {
        let mut idx = BTreeIndex::new("idx1".into(), "users".into(), "age".into());
        for i in 0..10 {
            idx.insert(Value::Int(i), RowId(i as u64));
        }

        let result = idx.scan(ScanOp::Gt, &Value::Int(5));
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_btree_remove() {
        let mut idx = BTreeIndex::new("idx1".into(), "users".into(), "id".into());
        idx.insert(Value::Int(1), RowId(0));
        idx.insert(Value::Int(1), RowId(1));
        idx.remove_by_row_id(&RowId(0));
        let result = idx.lookup_eq(&Value::Int(1));
        assert_eq!(result, vec![RowId(1)]);
    }
}

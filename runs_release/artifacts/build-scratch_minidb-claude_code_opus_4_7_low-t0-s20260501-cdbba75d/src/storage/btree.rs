//! Secondary B-tree-style index keyed by (key, row_id).
//!
//! The on-disk representation is part of the database snapshot — the index is
//! held in a `BTreeMap<IndexKey, Vec<u64>>` and serialized along with the rest
//! of the catalog/state. We expose B-tree-shaped operations (point lookup,
//! range scan) so callers do not depend on the in-memory representation.

use crate::types::Value;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// Order-preserving wrapper around `Value` so it can serve as a BTreeMap key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexKey(pub Value);

impl PartialEq for IndexKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for IndexKey {}

impl PartialOrd for IndexKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for IndexKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.0, &other.0) {
            (Value::Null, Value::Null) => Ordering::Equal,
            (Value::Null, _) => Ordering::Less,
            (_, Value::Null) => Ordering::Greater,
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Real(a), Value::Real(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Value::Int(a), Value::Real(b)) => {
                (*a as f64).partial_cmp(b).unwrap_or(Ordering::Equal)
            }
            (Value::Real(a), Value::Int(b)) => {
                a.partial_cmp(&(*b as f64)).unwrap_or(Ordering::Equal)
            }
            (Value::Text(a), Value::Text(b)) => a.cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            // mixed types: compare by tag order for total order
            (a, b) => format!("{:?}", a).cmp(&format!("{:?}", b)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BTreeIndex {
    pub map: BTreeMap<IndexKey, Vec<u64>>, // key -> row_ids
}

impl BTreeIndex {
    pub fn new() -> Self {
        BTreeIndex {
            map: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, key: Value, row_id: u64) {
        let entry = self.map.entry(IndexKey(key)).or_default();
        if !entry.contains(&row_id) {
            entry.push(row_id);
        }
    }

    pub fn remove(&mut self, key: &Value, row_id: u64) {
        let k = IndexKey(key.clone());
        if let Some(v) = self.map.get_mut(&k) {
            v.retain(|r| *r != row_id);
            if v.is_empty() {
                self.map.remove(&k);
            }
        }
    }

    pub fn lookup_eq(&self, key: &Value) -> Vec<u64> {
        self.map
            .get(&IndexKey(key.clone()))
            .cloned()
            .unwrap_or_default()
    }

    pub fn range(
        &self,
        lo: Option<&Value>,
        lo_inclusive: bool,
        hi: Option<&Value>,
        hi_inclusive: bool,
    ) -> Vec<u64> {
        use std::ops::Bound::*;
        let lo_b = match (lo, lo_inclusive) {
            (None, _) => Unbounded,
            (Some(v), true) => Included(IndexKey(v.clone())),
            (Some(v), false) => Excluded(IndexKey(v.clone())),
        };
        let hi_b = match (hi, hi_inclusive) {
            (None, _) => Unbounded,
            (Some(v), true) => Included(IndexKey(v.clone())),
            (Some(v), false) => Excluded(IndexKey(v.clone())),
        };
        let mut out = Vec::new();
        for (_, ids) in self.map.range((lo_b, hi_b)) {
            out.extend(ids.iter().copied());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_lookup() {
        let mut idx = BTreeIndex::new();
        idx.insert(Value::Int(1), 10);
        idx.insert(Value::Int(2), 20);
        idx.insert(Value::Int(1), 11);
        assert_eq!(idx.lookup_eq(&Value::Int(1)).len(), 2);
        assert_eq!(idx.lookup_eq(&Value::Int(2)), vec![20]);
    }

    #[test]
    fn range_scan() {
        let mut idx = BTreeIndex::new();
        for i in 1..=10 {
            idx.insert(Value::Int(i), i as u64);
        }
        let r = idx.range(Some(&Value::Int(3)), true, Some(&Value::Int(6)), false);
        assert_eq!(r, vec![3, 4, 5]);
    }

    #[test]
    fn remove_clears_entry() {
        let mut idx = BTreeIndex::new();
        idx.insert(Value::Int(1), 1);
        idx.remove(&Value::Int(1), 1);
        assert!(idx.lookup_eq(&Value::Int(1)).is_empty());
    }
}

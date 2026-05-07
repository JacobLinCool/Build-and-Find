//! Secondary index structure.
//!
//! The on-disk goal is a classic B-tree, but the seed project uses a
//! sorted map internally — the entries are keyed by
//! `(index_key, row_id)` so the same key may point to multiple rows
//! (non-unique indexes) without collisions.

use crate::error::Result;
use crate::storage::heap::RowId;
use crate::types::Value;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Composite key used by secondary indexes: the user-visible key plus the
/// owning row id so the map never sees duplicates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexKey {
    pub value: Value,
    pub row_id: RowId,
}

impl PartialEq for IndexKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for IndexKey {}

impl PartialOrd for IndexKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for IndexKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match crate::types::compare_values(&self.value, &other.value) {
            Some(ord) => ord.then(self.row_id.cmp(&other.row_id)),
            None => {
                // NULLs sort last, consistent with most engines.
                let a_null = self.value.is_null();
                let b_null = other.value.is_null();
                match (a_null, b_null) {
                    (true, true) => self.row_id.cmp(&other.row_id),
                    (true, false) => std::cmp::Ordering::Greater,
                    (false, true) => std::cmp::Ordering::Less,
                    (false, false) => self.row_id.cmp(&other.row_id),
                }
            }
        }
    }
}

/// Public trait for the secondary index store so callers don't depend on
/// the concrete representation.
pub trait Index {
    fn insert(&mut self, value: Value, row_id: RowId) -> Result<()>;
    fn remove(&mut self, value: &Value, row_id: RowId) -> Result<()>;
    fn lookup_eq(&self, value: &Value) -> Vec<RowId>;
    fn lookup_range(&self, lo: Option<&Value>, hi: Option<&Value>) -> Vec<RowId>;
    fn len(&self) -> usize;
}

/// Default in-memory implementation used everywhere in the seed.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BTreeIndex {
    entries: BTreeMap<IndexKey, ()>,
}

impl BTreeIndex {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Index for BTreeIndex {
    fn insert(&mut self, value: Value, row_id: RowId) -> Result<()> {
        self.entries.insert(IndexKey { value, row_id }, ());
        Ok(())
    }

    fn remove(&mut self, value: &Value, row_id: RowId) -> Result<()> {
        self.entries.remove(&IndexKey {
            value: value.clone(),
            row_id,
        });
        Ok(())
    }

    fn lookup_eq(&self, value: &Value) -> Vec<RowId> {
        self.entries
            .keys()
            .filter(|k| &k.value == value)
            .map(|k| k.row_id)
            .collect()
    }

    fn lookup_range(&self, lo: Option<&Value>, hi: Option<&Value>) -> Vec<RowId> {
        self.entries
            .keys()
            .filter(|k| {
                let lo_ok = match lo {
                    Some(v) => matches!(
                        crate::types::compare_values(&k.value, v),
                        Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                    ),
                    None => true,
                };
                let hi_ok = match hi {
                    Some(v) => matches!(
                        crate::types::compare_values(&k.value, v),
                        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                    ),
                    None => true,
                };
                lo_ok && hi_ok
            })
            .map(|k| k.row_id)
            .collect()
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_lookup_eq() {
        let mut idx = BTreeIndex::new();
        idx.insert(Value::Int(5), RowId(1)).unwrap();
        idx.insert(Value::Int(5), RowId(2)).unwrap();
        idx.insert(Value::Int(7), RowId(3)).unwrap();
        let mut hits = idx.lookup_eq(&Value::Int(5));
        hits.sort();
        assert_eq!(hits, vec![RowId(1), RowId(2)]);
    }
}

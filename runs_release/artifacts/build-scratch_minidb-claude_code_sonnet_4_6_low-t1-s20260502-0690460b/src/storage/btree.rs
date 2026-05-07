/// B-Tree secondary index keyed by (index_key, row_id)
/// Simple in-memory BTreeMap-backed implementation with persistence
use crate::error::{MiniDbError, Result};
use crate::types::Value;
use std::collections::BTreeMap;

pub type RowId = u64;

/// Key for the B-tree: (value, row_id) for uniqueness
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BTreeKey {
    pub value: Value,
    pub row_id: RowId,
}

pub struct BTree {
    pub entries: BTreeMap<BTreeKey, ()>,
}

impl BTree {
    pub fn new() -> Self {
        BTree {
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, value: Value, row_id: RowId) {
        self.entries.insert(BTreeKey { value, row_id }, ());
    }

    pub fn remove(&mut self, value: &Value, row_id: RowId) {
        self.entries.remove(&BTreeKey {
            value: value.clone(),
            row_id,
        });
    }

    /// Equality lookup: all row_ids matching value
    pub fn lookup_eq(&self, value: &Value) -> Vec<RowId> {
        let low = BTreeKey {
            value: value.clone(),
            row_id: 0,
        };
        let high = BTreeKey {
            value: value.clone(),
            row_id: u64::MAX,
        };
        self.entries
            .range(low..=high)
            .map(|(k, _)| k.row_id)
            .collect()
    }

    /// Range lookup: all row_ids with value in [low, high]
    pub fn lookup_range(&self, low: Option<&Value>, high: Option<&Value>) -> Vec<RowId> {
        let low_key = low.map(|v| BTreeKey {
            value: v.clone(),
            row_id: 0,
        });
        let high_key = high.map(|v| BTreeKey {
            value: v.clone(),
            row_id: u64::MAX,
        });
        match (low_key, high_key) {
            (Some(l), Some(h)) => self.entries.range(l..=h).map(|(k, _)| k.row_id).collect(),
            (Some(l), None) => self.entries.range(l..).map(|(k, _)| k.row_id).collect(),
            (None, Some(h)) => self.entries.range(..=h).map(|(k, _)| k.row_id).collect(),
            (None, None) => self.entries.keys().map(|k| k.row_id).collect(),
        }
    }

    pub fn contains(&self, value: &Value, row_id: RowId) -> bool {
        self.entries.contains_key(&BTreeKey {
            value: value.clone(),
            row_id,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.entries.len() as u64).to_le_bytes());
        for key in self.entries.keys() {
            let vbytes = key.value.to_bytes();
            out.extend_from_slice(&(vbytes.len() as u32).to_le_bytes());
            out.extend_from_slice(&vbytes);
            out.extend_from_slice(&key.row_id.to_le_bytes());
        }
        Ok(out)
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 8 {
            return Ok(BTree::new());
        }
        let count = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
        let mut pos = 8;
        let mut entries = BTreeMap::new();
        for _ in 0..count {
            if pos + 4 > data.len() {
                break;
            }
            let vlen = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            if pos + vlen + 8 > data.len() {
                break;
            }
            let (value, _) = Value::from_bytes(&data[pos..pos + vlen])?;
            pos += vlen;
            let row_id = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
            pos += 8;
            entries.insert(BTreeKey { value, row_id }, ());
        }
        Ok(BTree { entries })
    }
}

impl Default for BTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;

    #[test]
    fn test_btree_insert_lookup() {
        let mut bt = BTree::new();
        bt.insert(Value::Int(10), 1);
        bt.insert(Value::Int(20), 2);
        bt.insert(Value::Int(10), 3);

        let rows = bt.lookup_eq(&Value::Int(10));
        assert_eq!(rows.len(), 2);
        assert!(rows.contains(&1));
        assert!(rows.contains(&3));
    }

    #[test]
    fn test_btree_range() {
        let mut bt = BTree::new();
        for i in 0..10i64 {
            bt.insert(Value::Int(i), i as u64);
        }
        let rows = bt.lookup_range(Some(&Value::Int(3)), Some(&Value::Int(6)));
        assert_eq!(rows.len(), 4); // 3,4,5,6
    }

    #[test]
    fn test_btree_remove() {
        let mut bt = BTree::new();
        bt.insert(Value::Int(5), 1);
        bt.remove(&Value::Int(5), 1);
        assert!(bt.lookup_eq(&Value::Int(5)).is_empty());
    }

    #[test]
    fn test_btree_serialization() {
        let mut bt = BTree::new();
        bt.insert(Value::Int(1), 100);
        bt.insert(Value::Text("hello".into()), 200);
        let bytes = bt.to_bytes().unwrap();
        let bt2 = BTree::from_bytes(&bytes).unwrap();
        assert_eq!(bt2.lookup_eq(&Value::Int(1)), vec![100]);
    }
}

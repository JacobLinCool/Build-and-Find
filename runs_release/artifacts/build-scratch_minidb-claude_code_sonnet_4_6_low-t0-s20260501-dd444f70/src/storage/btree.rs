/// B+tree secondary index implementation.
/// Keys are serialized Values, values are RowIds (u64).
/// The tree is stored in-memory and can be persisted/loaded.
use crate::error::{MiniDbError, Result};
use crate::types::{RowId, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct BTree {
    /// (key, row_id) -> ()  (BTreeMap gives sorted order automatically)
    entries: BTreeMap<BTreeKey, ()>,
    pub unique: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BTreeKey {
    pub key: Value,
    pub row_id: RowId,
}

impl BTree {
    pub fn new(unique: bool) -> Self {
        BTree {
            entries: BTreeMap::new(),
            unique,
        }
    }

    pub fn insert(&mut self, key: Value, row_id: RowId) -> Result<()> {
        if self.unique {
            // Check if key already exists with different row_id
            let start = BTreeKey {
                key: key.clone(),
                row_id: 0,
            };
            let end = BTreeKey {
                key: key.clone(),
                row_id: u64::MAX,
            };
            let existing: Vec<_> = self.entries.range(start..=end).collect();
            if !existing.is_empty() {
                return Err(MiniDbError::DuplicateKey(format!(
                    "duplicate key {:?}",
                    key
                )));
            }
        }
        self.entries.insert(BTreeKey { key, row_id }, ());
        Ok(())
    }

    pub fn delete(&mut self, key: &Value, row_id: RowId) {
        let k = BTreeKey {
            key: key.clone(),
            row_id,
        };
        self.entries.remove(&k);
    }

    pub fn lookup_eq(&self, key: &Value) -> Vec<RowId> {
        let start = BTreeKey {
            key: key.clone(),
            row_id: 0,
        };
        let end = BTreeKey {
            key: key.clone(),
            row_id: u64::MAX,
        };
        self.entries
            .range(start..=end)
            .map(|(k, _)| k.row_id)
            .collect()
    }

    pub fn lookup_range(
        &self,
        low: Option<&Value>,
        low_inclusive: bool,
        high: Option<&Value>,
        high_inclusive: bool,
    ) -> Vec<RowId> {
        let mut result = Vec::new();
        for (k, _) in &self.entries {
            if k.key.is_null() {
                continue;
            }
            let low_ok = match low {
                None => true,
                Some(lo) => {
                    if low_inclusive {
                        matches!(
                            k.key.sql_cmp(lo),
                            Some(std::cmp::Ordering::Greater) | Some(std::cmp::Ordering::Equal)
                        )
                    } else {
                        matches!(k.key.sql_cmp(lo), Some(std::cmp::Ordering::Greater))
                    }
                }
            };
            let high_ok = match high {
                None => true,
                Some(hi) => {
                    if high_inclusive {
                        matches!(
                            k.key.sql_cmp(hi),
                            Some(std::cmp::Ordering::Less) | Some(std::cmp::Ordering::Equal)
                        )
                    } else {
                        matches!(k.key.sql_cmp(hi), Some(std::cmp::Ordering::Less))
                    }
                }
            };
            if low_ok && high_ok {
                result.push(k.row_id);
            }
        }
        result
    }

    pub fn all_entries(&self) -> Vec<(Value, RowId)> {
        self.entries
            .keys()
            .map(|k| (k.key.clone(), k.row_id))
            .collect()
    }

    /// Serialize to bytes for persistence
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let entries: Vec<_> = self.all_entries();
        buf.extend_from_slice(&(entries.len() as u64).to_le_bytes());
        buf.push(if self.unique { 1u8 } else { 0u8 });
        for (key, row_id) in &entries {
            let key_bytes = key.serialize();
            buf.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(&key_bytes);
            buf.extend_from_slice(&row_id.to_le_bytes());
        }
        buf
    }

    /// Deserialize from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        if data.len() < 9 {
            return Ok(BTree::new(false));
        }
        let num_entries = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
        let unique = data[8] != 0;
        let mut tree = BTree::new(unique);
        let mut pos = 9;

        for _ in 0..num_entries {
            if pos + 4 > data.len() {
                return Err(MiniDbError::Storage(
                    "btree deserialize: truncated key len".to_string(),
                ));
            }
            let key_len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            if pos + key_len + 8 > data.len() {
                return Err(MiniDbError::Storage(
                    "btree deserialize: truncated key data".to_string(),
                ));
            }
            let (key, _) = Value::deserialize(&data[pos..pos + key_len])?;
            pos += key_len;
            let row_id = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
            pos += 8;
            // Insert without unique check during load
            tree.entries.insert(BTreeKey { key, row_id }, ());
        }
        Ok(tree)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_btree_insert_lookup() {
        let mut tree = BTree::new(false);
        tree.insert(Value::Int(10), 1).unwrap();
        tree.insert(Value::Int(20), 2).unwrap();
        tree.insert(Value::Int(10), 3).unwrap(); // duplicate key, different row

        let results = tree.lookup_eq(&Value::Int(10));
        assert_eq!(results.len(), 2);
        assert!(results.contains(&1));
        assert!(results.contains(&3));
    }

    #[test]
    fn test_btree_unique_constraint() {
        let mut tree = BTree::new(true);
        tree.insert(Value::Int(1), 1).unwrap();
        assert!(tree.insert(Value::Int(1), 2).is_err());
    }

    #[test]
    fn test_btree_range_lookup() {
        let mut tree = BTree::new(false);
        for i in 0..10i64 {
            tree.insert(Value::Int(i), i as u64).unwrap();
        }

        let results = tree.lookup_range(Some(&Value::Int(3)), true, Some(&Value::Int(7)), false);
        assert_eq!(results.len(), 4); // 3, 4, 5, 6
    }

    #[test]
    fn test_btree_delete() {
        let mut tree = BTree::new(false);
        tree.insert(Value::Int(1), 10).unwrap();
        tree.insert(Value::Int(2), 20).unwrap();
        tree.delete(&Value::Int(1), 10);

        assert!(tree.lookup_eq(&Value::Int(1)).is_empty());
        assert_eq!(tree.lookup_eq(&Value::Int(2)), vec![20]);
    }

    #[test]
    fn test_btree_serialize_roundtrip() {
        let mut tree = BTree::new(false);
        tree.insert(Value::Int(1), 100).unwrap();
        tree.insert(Value::Text("hello".to_string()), 200).unwrap();
        tree.insert(Value::Real(3.14), 300).unwrap();

        let bytes = tree.serialize();
        let tree2 = BTree::deserialize(&bytes).unwrap();
        assert_eq!(tree2.len(), 3);
        assert_eq!(tree2.lookup_eq(&Value::Int(1)), vec![100]);
    }
}

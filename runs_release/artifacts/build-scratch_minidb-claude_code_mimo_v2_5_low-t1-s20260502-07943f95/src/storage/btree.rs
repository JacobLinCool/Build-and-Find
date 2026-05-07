use super::RowId;
use crate::error::{MiniDbError, Result};
use log::debug;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct BTreeIndex {
    pub name: String,
    pub table: String,
    pub column: String,
    entries: BTreeMap<IndexKey, Vec<RowId>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IndexKey {
    Int(i64),
    Text(String),
    Null,
}

impl IndexKey {
    pub fn from_value(val: &crate::types::Value) -> Self {
        match val {
            crate::types::Value::Int(v) => IndexKey::Int(*v),
            crate::types::Value::Text(v) => IndexKey::Text(v.clone()),
            crate::types::Value::Null => IndexKey::Null,
            crate::types::Value::Real(v) => IndexKey::Int(*v as i64),
            crate::types::Value::Bool(v) => IndexKey::Int(*v as i64),
        }
    }
}

#[derive(Debug, Clone)]
pub enum IndexPredicate {
    Eq(IndexKey),
    Gt(IndexKey),
    Ge(IndexKey),
    Lt(IndexKey),
    Le(IndexKey),
    Range {
        min: Option<IndexKey>,
        max: Option<IndexKey>,
        min_inclusive: bool,
        max_inclusive: bool,
    },
}

impl BTreeIndex {
    pub fn new(name: &str, table: &str, column: &str) -> Self {
        Self {
            name: name.to_string(),
            table: table.to_string(),
            column: column.to_string(),
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, key: IndexKey, row_id: RowId) {
        self.entries.entry(key).or_default().push(row_id);
    }

    pub fn remove(&mut self, key: &IndexKey, row_id: RowId) {
        if let Some(ids) = self.entries.get_mut(key) {
            ids.retain(|id| *id != row_id);
            if ids.is_empty() {
                self.entries.remove(key);
            }
        }
    }

    pub fn lookup(&self, predicate: &IndexPredicate) -> Vec<RowId> {
        let mut result = Vec::new();
        match predicate {
            IndexPredicate::Eq(key) => {
                if let Some(ids) = self.entries.get(key) {
                    result.extend(ids.iter().copied());
                }
            }
            IndexPredicate::Gt(key) => {
                for (k, ids) in self.entries.range((
                    std::ops::Bound::Excluded(key.clone()),
                    std::ops::Bound::Unbounded,
                )) {
                    result.extend(ids.iter().copied());
                }
            }
            IndexPredicate::Ge(key) => {
                for (k, ids) in self.entries.range((
                    std::ops::Bound::Included(key.clone()),
                    std::ops::Bound::Unbounded,
                )) {
                    result.extend(ids.iter().copied());
                }
            }
            IndexPredicate::Lt(key) => {
                for (k, ids) in self.entries.range((
                    std::ops::Bound::Unbounded,
                    std::ops::Bound::Excluded(key.clone()),
                )) {
                    result.extend(ids.iter().copied());
                }
            }
            IndexPredicate::Le(key) => {
                for (k, ids) in self.entries.range((
                    std::ops::Bound::Unbounded,
                    std::ops::Bound::Included(key.clone()),
                )) {
                    result.extend(ids.iter().copied());
                }
            }
            IndexPredicate::Range {
                min,
                max,
                min_inclusive,
                max_inclusive,
            } => {
                let start = match min {
                    Some(k) if *min_inclusive => std::ops::Bound::Included(k.clone()),
                    Some(k) => std::ops::Bound::Excluded(k.clone()),
                    None => std::ops::Bound::Unbounded,
                };
                let end = match max {
                    Some(k) if *max_inclusive => std::ops::Bound::Included(k.clone()),
                    Some(k) => std::ops::Bound::Excluded(k.clone()),
                    None => std::ops::Bound::Unbounded,
                };
                for (k, ids) in self.entries.range((start, end)) {
                    result.extend(ids.iter().copied());
                }
            }
        }
        result
    }

    pub fn all_entries(&self) -> impl Iterator<Item = (&IndexKey, &Vec<RowId>)> {
        self.entries.iter()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        for (key, ids) in &self.entries {
            let key_bytes = match key {
                IndexKey::Int(v) => {
                    let mut buf = vec![0x01u8];
                    buf.extend_from_slice(&v.to_be_bytes());
                    buf
                }
                IndexKey::Text(s) => {
                    let mut buf = vec![0x02u8];
                    buf.extend_from_slice(&(s.len() as u16).to_be_bytes());
                    buf.extend_from_slice(s.as_bytes());
                    buf
                }
                IndexKey::Null => vec![0x00],
            };
            data.extend_from_slice(&(key_bytes.len() as u16).to_be_bytes());
            data.extend_from_slice(&key_bytes);
            data.extend_from_slice(&(ids.len() as u32).to_be_bytes());
            for id in ids {
                data.extend_from_slice(&id.0.to_be_bytes());
            }
        }
        data
    }

    pub fn deserialize(data: &[u8]) -> Result<Self> {
        let mut entries = BTreeMap::new();
        let mut pos = 0;
        while pos + 2 < data.len() {
            let key_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            if pos + key_len > data.len() {
                break;
            }
            let key_bytes = &data[pos..pos + key_len];
            pos += key_len;
            let key = match key_bytes[0] {
                0x01 => {
                    let v = i64::from_be_bytes(
                        key_bytes[1..9]
                            .try_into()
                            .map_err(|_| MiniDbError::Storage("Invalid int key".into()))?,
                    );
                    IndexKey::Int(v)
                }
                0x02 => {
                    let len = u16::from_be_bytes([key_bytes[1], key_bytes[2]]) as usize;
                    let s = String::from_utf8(key_bytes[3..3 + len].to_vec())
                        .map_err(|e| MiniDbError::Storage(e.to_string()))?;
                    IndexKey::Text(s)
                }
                _ => IndexKey::Null,
            };
            let id_count =
                u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                    as usize;
            pos += 4;
            let mut ids = Vec::new();
            for _ in 0..id_count {
                let id = u64::from_be_bytes(
                    data[pos..pos + 8]
                        .try_into()
                        .map_err(|_| MiniDbError::Storage("Invalid row id".into()))?,
                );
                ids.push(RowId(id));
                pos += 8;
            }
            entries.insert(key, ids);
        }
        Ok(Self {
            name: String::new(),
            table: String::new(),
            column: String::new(),
            entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_btree_insert_and_lookup() {
        let mut idx = BTreeIndex::new("idx", "t", "c");
        idx.insert(IndexKey::Int(5), RowId(1));
        idx.insert(IndexKey::Int(3), RowId(2));
        idx.insert(IndexKey::Int(7), RowId(3));

        let result = idx.lookup(&IndexPredicate::Eq(IndexKey::Int(5)));
        assert_eq!(result, vec![RowId(1)]);

        let result = idx.lookup(&IndexPredicate::Gt(IndexKey::Int(4)));
        assert_eq!(result, vec![RowId(1), RowId(3)]); // 5, 7 in order
    }

    #[test]
    fn test_btree_range() {
        let mut idx = BTreeIndex::new("idx", "t", "c");
        for i in 0..10 {
            idx.insert(IndexKey::Int(i), RowId(i as u64));
        }
        let result = idx.lookup(&IndexPredicate::Range {
            min: Some(IndexKey::Int(3)),
            max: Some(IndexKey::Int(7)),
            min_inclusive: true,
            max_inclusive: false,
        });
        assert_eq!(result.len(), 4); // 3, 4, 5, 6
    }

    #[test]
    fn test_btree_remove() {
        let mut idx = BTreeIndex::new("idx", "t", "c");
        idx.insert(IndexKey::Int(5), RowId(1));
        idx.insert(IndexKey::Int(5), RowId(2));
        idx.remove(&IndexKey::Int(5), RowId(1));
        let result = idx.lookup(&IndexPredicate::Eq(IndexKey::Int(5)));
        assert_eq!(result, vec![RowId(2)]);
    }

    #[test]
    fn test_btree_serialize_deserialize() {
        let mut idx = BTreeIndex::new("idx", "t", "c");
        idx.insert(IndexKey::Int(5), RowId(1));
        idx.insert(IndexKey::Text("hello".into()), RowId(2));
        let data = idx.serialize();
        let mut idx2 = BTreeIndex::deserialize(&data).unwrap();
        idx2.name = "idx".into();
        idx2.table = "t".into();
        idx2.column = "c".into();
        assert_eq!(
            idx2.lookup(&IndexPredicate::Eq(IndexKey::Int(5))),
            vec![RowId(1)]
        );
    }
}

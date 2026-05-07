//! Persistent B-tree secondary index.
//!
//! The on-disk representation is the serialized contents of the in-memory
//! ordered map, so all search operations run against an in-memory copy.
//! Keys are `(index_key, row_id)` pairs, and lookups support equality
//! probes as well as range scans bounded by inclusive endpoints.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{MiniDbError, Result};
use crate::types::Value;

use super::page::RowId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexKey(pub Value, pub RowId);

impl IndexKey {
    pub fn new(value: Value, row: RowId) -> Self {
        Self(value, row)
    }
}

impl Eq for IndexKey {}

impl Ord for IndexKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.0.cmp_total(&other.0) {
            std::cmp::Ordering::Equal => self.1.cmp(&other.1),
            o => o,
        }
    }
}

impl PartialOrd for IndexKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// On-disk B-tree: keys are (Value, RowId), values are unit (the row id is the payload).
#[derive(Default, Serialize, Deserialize)]
pub struct BTree {
    entries: BTreeMap<IndexKey, ()>,
}

impl BTree {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, key: Value, row: RowId) -> Result<()> {
        let k = IndexKey::new(key, row);
        if self.entries.insert(k, ()).is_some() {
            // Idempotent insert is fine.
        }
        Ok(())
    }

    pub fn remove(&mut self, key: &Value, row: RowId) -> bool {
        let k = IndexKey::new(key.clone(), row);
        self.entries.remove(&k).is_some()
    }

    /// Look up all row ids whose key equals `value`.
    pub fn lookup_eq(&self, value: &Value) -> Vec<RowId> {
        let lower = IndexKey::new(value.clone(), RowId::new(super::PageId::NULL, 0));
        let upper = IndexKey::new(value.clone(), RowId::new(super::PageId(u64::MAX), u16::MAX));
        self.entries
            .range(lower..=upper)
            .map(|(k, _)| k.1)
            .collect()
    }

    /// Range scan with optional inclusive bounds.
    pub fn lookup_range(
        &self,
        lower: Option<(&Value, bool)>,
        upper: Option<(&Value, bool)>,
    ) -> Vec<RowId> {
        // Filter the full tree by predicate to avoid building exotic Bound types
        // for the (Value, RowId) composite key.
        self.entries
            .keys()
            .filter(|k| {
                if let Some((lv, inclusive)) = lower {
                    let cmp = k.0.cmp_total(lv);
                    if cmp == std::cmp::Ordering::Less {
                        return false;
                    }
                    if cmp == std::cmp::Ordering::Equal && !inclusive {
                        return false;
                    }
                }
                if let Some((uv, inclusive)) = upper {
                    let cmp = k.0.cmp_total(uv);
                    if cmp == std::cmp::Ordering::Greater {
                        return false;
                    }
                    if cmp == std::cmp::Ordering::Equal && !inclusive {
                        return false;
                    }
                }
                true
            })
            .map(|k| k.1)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// File-backed wrapper that loads the B-tree on construction and writes it
/// back atomically when persisted.
pub struct PersistentBTree {
    path: PathBuf,
    pub tree: BTree,
}

impl PersistentBTree {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let tree = if path.exists() {
            let mut buf = Vec::new();
            File::open(&path)?.read_to_end(&mut buf)?;
            if buf.is_empty() {
                BTree::new()
            } else {
                bincode::deserialize::<BTree>(&buf)
                    .map_err(|e| MiniDbError::storage(format!("corrupt index {:?}: {}", path, e)))?
            }
        } else {
            BTree::new()
        };
        Ok(Self { path, tree })
    }

    pub fn persist(&self) -> Result<()> {
        let bytes = bincode::serialize(&self.tree)?;
        let tmp = self.path.with_extension("idx.tmp");
        {
            let mut f = File::create(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn delete_file(self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::PageId;
    use tempfile::TempDir;

    #[test]
    fn insert_and_lookup_eq() {
        let mut t = BTree::new();
        t.insert(Value::Int(1), RowId::new(PageId(1), 0)).unwrap();
        t.insert(Value::Int(1), RowId::new(PageId(1), 1)).unwrap();
        t.insert(Value::Int(2), RowId::new(PageId(1), 2)).unwrap();
        let r = t.lookup_eq(&Value::Int(1));
        assert_eq!(r.len(), 2);
        let r = t.lookup_eq(&Value::Int(2));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn range_scan_inclusive_bounds() {
        let mut t = BTree::new();
        for i in 1..=10 {
            t.insert(Value::Int(i), RowId::new(PageId(1), i as u16))
                .unwrap();
        }
        let v = t.lookup_range(Some((&Value::Int(3), true)), Some((&Value::Int(5), true)));
        assert_eq!(v.len(), 3);
        let v = t.lookup_range(Some((&Value::Int(3), false)), Some((&Value::Int(5), true)));
        assert_eq!(v.len(), 2);
        let v = t.lookup_range(None, Some((&Value::Int(2), true)));
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn persistence_roundtrip() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("idx");
        let mut t = PersistentBTree::open(&p).unwrap();
        t.tree
            .insert(Value::Text("a".into()), RowId::new(PageId(1), 0))
            .unwrap();
        t.persist().unwrap();
        let t2 = PersistentBTree::open(&p).unwrap();
        assert_eq!(t2.tree.len(), 1);
    }

    #[test]
    fn remove_clears_entry() {
        let mut t = BTree::new();
        let row = RowId::new(PageId(1), 0);
        t.insert(Value::Int(1), row).unwrap();
        assert!(t.remove(&Value::Int(1), row));
        assert!(t.is_empty());
    }
}

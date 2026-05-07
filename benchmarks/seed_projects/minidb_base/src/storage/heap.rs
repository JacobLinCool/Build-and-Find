//! Row-oriented heap file.
//!
//! The heap is the primary physical store for table data. Each table owns
//! its own heap. Rows are addressed by a monotonically increasing
//! [`RowId`]. The in-memory implementation is backed by a `Vec`; this is
//! enough to drive the planner, executor, and higher layers while keeping
//! the seed project small.

use crate::error::{MiniDbError, Result};
use crate::types::Row;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Strongly typed row identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RowId(pub u64);

impl std::fmt::Display for RowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "row#{}", self.0)
    }
}

/// Simple row heap. Deletion is a tombstone — the `RowId` is never reused.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Heap {
    rows: BTreeMap<RowId, Row>,
    next_id: u64,
}

impl Heap {
    pub fn new() -> Self {
        Self {
            rows: BTreeMap::new(),
            next_id: 1,
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Insert a row and return its freshly allocated `RowId`.
    pub fn insert(&mut self, row: Row) -> RowId {
        let id = RowId(self.next_id);
        self.next_id += 1;
        self.rows.insert(id, row);
        id
    }

    /// Overwrite an existing row.
    pub fn update(&mut self, id: RowId, row: Row) -> Result<()> {
        if !self.rows.contains_key(&id) {
            return Err(MiniDbError::Storage(format!(
                "update: {id} does not exist"
            )));
        }
        self.rows.insert(id, row);
        Ok(())
    }

    /// Remove a row. Returns the removed value on success.
    pub fn delete(&mut self, id: RowId) -> Result<Row> {
        self.rows
            .remove(&id)
            .ok_or_else(|| MiniDbError::Storage(format!("delete: {id} does not exist")))
    }

    pub fn get(&self, id: RowId) -> Option<&Row> {
        self.rows.get(&id)
    }

    /// Iterate `(RowId, &Row)` in ascending key order (scan order).
    pub fn iter(&self) -> impl Iterator<Item = (RowId, &Row)> {
        self.rows.iter().map(|(k, v)| (*k, v))
    }

    /// Collect all `(RowId, Row)` pairs. Useful for operators that buffer
    /// their input.
    pub fn snapshot(&self) -> Vec<(RowId, Row)> {
        self.rows.iter().map(|(k, v)| (*k, v.clone())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;

    #[test]
    fn insert_get_delete() {
        let mut h = Heap::new();
        let id = h.insert(vec![Value::Int(7)]);
        assert_eq!(h.get(id).unwrap().len(), 1);
        h.delete(id).unwrap();
        assert!(h.get(id).is_none());
    }
}

use crate::storage::heap::RowId;
use crate::types::{Value, ValueKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BTreeIndex {
    pub name: String,
    pub table: String,
    pub column: String,
    pub entries: BTreeMap<ValueKey, BTreeSet<RowId>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeOp {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
}

impl BTreeIndex {
    pub fn new(name: String, table: String, column: String) -> Self {
        Self {
            name,
            table,
            column,
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, value: &Value, row_id: RowId) {
        self.entries.entry(value.key()).or_default().insert(row_id);
    }

    pub fn remove(&mut self, value: &Value, row_id: RowId) {
        let key = value.key();
        if let Some(set) = self.entries.get_mut(&key) {
            set.remove(&row_id);
            if set.is_empty() {
                self.entries.remove(&key);
            }
        }
    }

    pub fn scan(&self, op: RangeOp, value: &Value) -> Vec<RowId> {
        let key = value.key();
        let iter: Box<dyn Iterator<Item = (&ValueKey, &BTreeSet<RowId>)> + '_> = match op {
            RangeOp::Eq => Box::new(
                self.entries
                    .range((Bound::Included(&key), Bound::Included(&key))),
            ),
            RangeOp::Lt => Box::new(
                self.entries
                    .range((Bound::Unbounded, Bound::Excluded(&key))),
            ),
            RangeOp::Le => Box::new(
                self.entries
                    .range((Bound::Unbounded, Bound::Included(&key))),
            ),
            RangeOp::Gt => Box::new(
                self.entries
                    .range((Bound::Excluded(&key), Bound::Unbounded)),
            ),
            RangeOp::Ge => Box::new(
                self.entries
                    .range((Bound::Included(&key), Bound::Unbounded)),
            ),
        };
        iter.flat_map(|(_, ids)| ids.iter().copied()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_equal_keys() {
        let mut idx = BTreeIndex::new("i".into(), "t".into(), "c".into());
        idx.insert(&Value::Int(3), 10);
        assert_eq!(idx.scan(RangeOp::Eq, &Value::Int(3)), vec![10]);
    }
}

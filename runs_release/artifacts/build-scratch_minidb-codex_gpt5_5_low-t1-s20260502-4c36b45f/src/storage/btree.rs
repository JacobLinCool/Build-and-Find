use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::types::{RowId, Value};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BTreeIndex {
    map: BTreeMap<String, BTreeSet<RowId>>,
}

impl BTreeIndex {
    pub fn insert(&mut self, value: Value, row_id: RowId) {
        self.map.entry(value.key()).or_default().insert(row_id);
    }
    pub fn remove(&mut self, value: &Value, row_id: RowId) {
        if let Some(set) = self.map.get_mut(&value.key()) {
            set.remove(&row_id);
        }
    }
    pub fn eq(&self, value: &Value) -> Vec<RowId> {
        self.map
            .get(&value.key())
            .map(|s| s.iter().copied().collect())
            .map_or_else(Vec::new, |v| v)
    }
    pub fn all(&self) -> Vec<RowId> {
        self.map.values().flat_map(|s| s.iter().copied()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexes_key_to_row() {
        let mut b = BTreeIndex::default();
        b.insert(Value::Int(1), 7);
        assert_eq!(b.eq(&Value::Int(1)), vec![7]);
    }
}

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::storage::heap::RowId;
use crate::types::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BTreeIndex {
    pub map: BTreeMap<Value, BTreeSet<RowId>>,
}

impl BTreeIndex {
    pub fn insert(&mut self, key: Value, row_id: RowId) {
        self.map.entry(key).or_default().insert(row_id);
    }

    pub fn remove(&mut self, key: &Value, row_id: RowId) {
        if let Some(set) = self.map.get_mut(key) {
            set.remove(&row_id);
            if set.is_empty() {
                self.map.remove(key);
            }
        }
    }

    pub fn exact(&self, key: &Value) -> Vec<RowId> {
        self.map
            .get(key)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn range(
        &self,
        lower: Option<(&Value, bool)>,
        upper: Option<(&Value, bool)>,
    ) -> Vec<RowId> {
        let mut out = Vec::new();
        let range = match (lower, upper) {
            (Some((low, low_inc)), Some((high, high_inc))) => self.map.range((
                if low_inc {
                    std::ops::Bound::Included(low)
                } else {
                    std::ops::Bound::Excluded(low)
                },
                if high_inc {
                    std::ops::Bound::Included(high)
                } else {
                    std::ops::Bound::Excluded(high)
                },
            )),
            (Some((low, low_inc)), None) => self.map.range((
                if low_inc {
                    std::ops::Bound::Included(low)
                } else {
                    std::ops::Bound::Excluded(low)
                },
                std::ops::Bound::Unbounded,
            )),
            (None, Some((high, high_inc))) => self.map.range((
                std::ops::Bound::Unbounded,
                if high_inc {
                    std::ops::Bound::Included(high)
                } else {
                    std::ops::Bound::Excluded(high)
                },
            )),
            (None, None) => self.map.range::<Value, _>(..),
        };
        for (_, ids) in range {
            out.extend(ids.iter().copied());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_ranges() {
        let mut idx = BTreeIndex::default();
        idx.insert(Value::Int(1), RowId(1));
        idx.insert(Value::Int(2), RowId(2));
        assert_eq!(idx.exact(&Value::Int(1)), vec![RowId(1)]);
    }
}

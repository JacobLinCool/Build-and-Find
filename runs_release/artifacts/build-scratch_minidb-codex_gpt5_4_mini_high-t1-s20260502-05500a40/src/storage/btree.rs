use crate::types::Value;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

pub type RowId = u64;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IndexKey {
    Null,
    Bool(bool),
    Int(i64),
    Real(u64),
    Text(String),
}

impl IndexKey {
    pub fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(v) => Self::Bool(*v),
            Value::Int(v) => Self::Int(*v),
            Value::Real(v) => Self::Real(v.to_bits()),
            Value::Text(v) => Self::Text(v.clone()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BTreeIndex {
    entries: BTreeMap<(IndexKey, RowId), ()>,
}

impl BTreeIndex {
    pub fn insert(&mut self, key: IndexKey, row_id: RowId) {
        self.entries.insert((key, row_id), ());
    }

    pub fn remove(&mut self, key: &IndexKey, row_id: RowId) {
        self.entries.remove(&(key.clone(), row_id));
    }

    pub fn eq_search(&self, key: &IndexKey) -> Vec<RowId> {
        self.entries
            .range((key.clone(), 0)..=(key.clone(), RowId::MAX))
            .map(|((_, row_id), _)| *row_id)
            .collect()
    }

    pub fn range_search(
        &self,
        lower: Option<(&IndexKey, bool)>,
        upper: Option<(&IndexKey, bool)>,
    ) -> Vec<RowId> {
        let lower_bound = match lower {
            Some((key, inclusive)) => {
                if inclusive {
                    Bound::Included((key.clone(), 0))
                } else {
                    Bound::Excluded((key.clone(), RowId::MAX))
                }
            }
            None => Bound::Unbounded,
        };
        let upper_bound = match upper {
            Some((key, inclusive)) => {
                if inclusive {
                    Bound::Included((key.clone(), RowId::MAX))
                } else {
                    Bound::Excluded((key.clone(), 0))
                }
            }
            None => Bound::Unbounded,
        };
        self.entries
            .range((lower_bound, upper_bound))
            .map(|((_, row_id), _)| *row_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_eq_search() {
        let mut index = BTreeIndex::default();
        index.insert(IndexKey::Int(7), 1);
        index.insert(IndexKey::Int(7), 2);
        assert_eq!(index.eq_search(&IndexKey::Int(7)), vec![1, 2]);
    }
}

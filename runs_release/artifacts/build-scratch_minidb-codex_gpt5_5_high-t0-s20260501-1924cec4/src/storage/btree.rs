use crate::types::Value;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BTreeIndex {
    pub name: String,
    pub table: String,
    pub column: String,
    pub map: BTreeMap<String, Vec<u64>>,
}

impl BTreeIndex {
    pub fn new(name: String, table: String, column: String) -> Self {
        Self {
            name,
            table,
            column,
            map: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, value: &Value, row_id: u64) {
        self.map.entry(value.key()).or_default().push(row_id);
    }

    pub fn equals(&self, value: &Value) -> Vec<u64> {
        self.map.get(&value.key()).cloned().unwrap_or_default()
    }

    pub fn range(&self, op: &str, value: &Value) -> Vec<u64> {
        let key = value.key();
        self.map
            .iter()
            .filter(|(candidate, _)| match op {
                ">" => *candidate > &key,
                ">=" => *candidate >= &key,
                "<" => *candidate < &key,
                "<=" => *candidate <= &key,
                _ => false,
            })
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_equality_lookup() {
        let mut idx = BTreeIndex::new("i".into(), "t".into(), "id".into());
        idx.insert(&Value::Int(4), 9);
        assert_eq!(idx.equals(&Value::Int(4)), vec![9]);
    }
}

use serde::{Deserialize, Serialize};

use crate::types::Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IndexData {
    pub entries: std::collections::BTreeMap<(Value, u64), ()>,
}

impl IndexData {
    pub fn insert(&mut self, key: Value, row_id: u64) {
        self.entries.insert((key, row_id), ());
    }
    pub fn remove(&mut self, key: &Value, row_id: u64) {
        self.entries.remove(&(key.clone(), row_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inserts_and_removes() {
        let mut idx = IndexData::default();
        idx.insert(Value::Int(1), 7);
        assert!(idx.entries.contains_key(&(Value::Int(1), 7)));
        idx.remove(&Value::Int(1), 7);
        assert!(idx.entries.is_empty());
    }
}

use crate::{storage::RowId, types::Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct BTreeIndex {
    map: BTreeMap<String, Vec<RowId>>,
}

impl BTreeIndex {
    pub fn insert(&mut self, key: Value, row_id: RowId) {
        self.map.entry(key.key()).or_default().push(row_id);
    }
    pub fn eq(&self, key: &Value) -> Vec<RowId> {
        self.map
            .get(&key.key())
            .cloned()
            .map_or_else(Vec::new, |v| v)
    }
    pub fn range(&self, low: Option<&Value>, high: Option<&Value>) -> Vec<RowId> {
        let lo = low.map(Value::key).map_or_else(String::new, |v| v);
        let hi = high
            .map(Value::key)
            .map_or_else(|| "\u{10ffff}".to_string(), |v| v);
        self.map
            .range(lo..=hi)
            .flat_map(|(_, ids)| ids.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn equality_lookup() {
        let mut b = BTreeIndex::default();
        b.insert(Value::Int(1), 7);
        assert_eq!(b.eq(&Value::Int(1)), vec![7]);
    }
}

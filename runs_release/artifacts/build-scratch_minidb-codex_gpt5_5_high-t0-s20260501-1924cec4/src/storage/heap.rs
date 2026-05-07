use crate::types::Value;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HeapTable {
    pub rows: Vec<StoredRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredRow {
    pub id: u64,
    pub values: Vec<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_row_ids_with_values() {
        let row = StoredRow {
            id: 7,
            values: vec![Value::Int(1)],
        };
        assert_eq!(row.id, 7);
    }
}

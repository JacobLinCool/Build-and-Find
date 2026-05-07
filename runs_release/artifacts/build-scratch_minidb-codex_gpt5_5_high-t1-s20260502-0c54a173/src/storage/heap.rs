use crate::types::RowValues;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type RowId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredRow {
    pub id: RowId,
    pub values: RowValues,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TableData {
    pub next_row_id: RowId,
    pub rows: BTreeMap<RowId, StoredRow>,
}

impl TableData {
    pub fn insert(&mut self, values: RowValues) -> RowId {
        let id = self.next_row_id;
        self.next_row_id = self.next_row_id.saturating_add(1);
        self.rows.insert(id, StoredRow { id, values });
        id
    }

    pub fn delete(&mut self, row_id: RowId) -> Option<StoredRow> {
        self.rows.remove(&row_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;

    #[test]
    fn row_ids_increase() {
        let mut table = TableData::default();
        assert_eq!(table.insert(vec![Value::Int(1)]), 0);
        assert_eq!(table.insert(vec![Value::Int(2)]), 1);
    }
}

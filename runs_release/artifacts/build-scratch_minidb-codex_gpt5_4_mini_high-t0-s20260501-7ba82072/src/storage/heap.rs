use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::types::Row;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RowId(pub u64);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HeapTable {
    pub(crate) next_row_id: u64,
    pub(crate) rows: BTreeMap<RowId, Row>,
}

impl HeapTable {
    pub fn insert(&mut self, row: Row) -> RowId {
        let row_id = RowId(self.next_row_id);
        self.next_row_id += 1;
        self.rows.insert(row_id, row);
        row_id
    }

    pub fn delete(&mut self, row_id: RowId) -> Option<Row> {
        self.rows.remove(&row_id)
    }

    pub fn get(&self, row_id: RowId) -> Option<&Row> {
        self.rows.get(&row_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (RowId, &Row)> {
        self.rows.iter().map(|(row_id, row)| (*row_id, row))
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;

    #[test]
    fn heap_inserts_and_reads() {
        let mut heap = HeapTable::default();
        let id = heap.insert(vec![Value::Int(1)]);
        assert_eq!(heap.get(id).unwrap()[0], Value::Int(1));
    }
}

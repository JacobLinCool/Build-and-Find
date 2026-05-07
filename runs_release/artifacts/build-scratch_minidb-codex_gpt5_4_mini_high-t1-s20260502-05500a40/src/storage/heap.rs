use crate::catalog::{IndexSchema, TableSchema};
use crate::error::{MiniDbError, Result};
use crate::storage::btree::{BTreeIndex, IndexKey, RowId};
use crate::types::Row;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeapTable {
    pub schema: TableSchema,
    pub rows: BTreeMap<RowId, Row>,
    pub next_row_id: RowId,
    pub indexes: BTreeMap<String, HeapIndex>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeapIndex {
    pub schema: IndexSchema,
    pub data: BTreeIndex,
}

impl HeapTable {
    pub fn new(schema: TableSchema) -> Self {
        Self {
            schema,
            rows: BTreeMap::new(),
            next_row_id: 1,
            indexes: BTreeMap::new(),
        }
    }

    pub fn insert_row(&mut self, row: Row) -> Result<RowId> {
        let row_id = self.next_row_id;
        self.next_row_id += 1;
        self.rows.insert(row_id, row.clone());
        self.rebuild_indexes_for_row(row_id, &row);
        Ok(row_id)
    }

    pub fn delete_row(&mut self, row_id: RowId) -> Option<Row> {
        let row = self.rows.remove(&row_id)?;
        self.remove_indexes_for_row(row_id, &row);
        Some(row)
    }

    pub fn update_row(&mut self, row_id: RowId, row: Row) {
        if let Some(old) = self.rows.insert(row_id, row.clone()) {
            self.remove_indexes_for_row(row_id, &old);
        }
        self.rebuild_indexes_for_row(row_id, &row);
    }

    pub fn add_index(&mut self, schema: IndexSchema) -> Result<()> {
        let mut index = HeapIndex {
            schema: schema.clone(),
            data: BTreeIndex::default(),
        };
        let col_idx = self.schema.column_index(&schema.column).ok_or_else(|| {
            MiniDbError::runtime(format!("unknown indexed column {}", schema.column))
        })?;
        for (row_id, row) in &self.rows {
            index
                .data
                .insert(IndexKey::from_value(&row.values[col_idx]), *row_id);
        }
        self.indexes.insert(schema.name, index);
        Ok(())
    }

    pub fn remove_index(&mut self, name: &str) {
        self.indexes.remove(name);
    }

    pub fn rebuild_indexes_for_row(&mut self, row_id: RowId, row: &Row) {
        for index in self.indexes.values_mut() {
            if let Some(col_idx) = self.schema.column_index(&index.schema.column) {
                index
                    .data
                    .insert(IndexKey::from_value(&row.values[col_idx]), row_id);
            }
        }
    }

    fn remove_indexes_for_row(&mut self, row_id: RowId, row: &Row) {
        for index in self.indexes.values_mut() {
            if let Some(col_idx) = self.schema.column_index(&index.schema.column) {
                index
                    .data
                    .remove(&IndexKey::from_value(&row.values[col_idx]), row_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;
    use crate::types::{Column, ColumnType};

    #[test]
    fn inserts_rows_and_updates_index() {
        let schema = TableSchema {
            name: "t".into(),
            columns: vec![Column::new("id", ColumnType::Int)],
            checks: vec![],
        };
        let mut table = HeapTable::new(schema);
        table
            .add_index(IndexSchema {
                name: "idx".into(),
                table: "t".into(),
                column: "id".into(),
            })
            .unwrap();
        table.insert_row(Row::new(vec![Value::Int(1)])).unwrap();
        let idx = table.indexes.get("idx").unwrap();
        assert_eq!(idx.data.eq_search(&IndexKey::Int(1)), vec![1]);
    }
}

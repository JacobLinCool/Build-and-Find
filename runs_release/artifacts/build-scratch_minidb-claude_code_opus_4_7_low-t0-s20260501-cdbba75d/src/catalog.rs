//! Schema and index metadata, plus the in-memory representation of table rows.
//!
//! The whole catalog (schemas, indexes, table data) is bincode-serialized and
//! persisted to pages by the `storage::heap::PageManager`. WAL records are
//! applied on top of the loaded snapshot during recovery.

use crate::error::{MiniDbError, Result};
use crate::storage::btree::BTreeIndex;
use crate::types::{Row, Schema, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Catalog {
    pub tables: BTreeMap<String, TableMeta>,
    pub indexes: BTreeMap<String, IndexMeta>,
    /// Stored Check expression source-text (re-parsed at load) per table.
    pub checks: BTreeMap<String, Vec<String>>,
    pub next_row_id: u64,
    pub next_txn_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableMeta {
    pub name: String,
    pub schema: Schema,
    /// Map row_id -> row.
    pub rows: BTreeMap<u64, Row>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    pub name: String,
    pub table: String,
    pub column: String,
    pub btree: BTreeIndex,
}

impl Catalog {
    pub fn new() -> Self {
        Catalog::default()
    }

    pub fn ensure_table_exists(&self, name: &str) -> Result<&TableMeta> {
        self.tables
            .get(&name.to_ascii_lowercase())
            .or_else(|| self.tables.get(name))
            .ok_or_else(|| MiniDbError::Catalog(format!("no such table: {}", name)))
    }

    pub fn get_table(&self, name: &str) -> Option<&TableMeta> {
        self.tables
            .get(&name.to_ascii_lowercase())
            .or_else(|| self.tables.get(name))
    }

    pub fn get_table_mut(&mut self, name: &str) -> Option<&mut TableMeta> {
        let key = if self.tables.contains_key(&name.to_ascii_lowercase()) {
            name.to_ascii_lowercase()
        } else {
            name.to_string()
        };
        self.tables.get_mut(&key)
    }

    pub fn next_row_id(&mut self) -> u64 {
        let id = self.next_row_id;
        self.next_row_id += 1;
        id
    }

    pub fn allocate_txn(&mut self) -> u64 {
        let id = self.next_txn_id + 1;
        self.next_txn_id = id;
        id
    }

    /// Find an index whose (table,column) match the given pair, returning the index name.
    pub fn find_index_for(&self, table: &str, column: &str) -> Option<&IndexMeta> {
        self.indexes
            .values()
            .find(|i| i.table.eq_ignore_ascii_case(table) && i.column.eq_ignore_ascii_case(column))
    }
}

/// Helper to look up a column index in a table's schema, returning a typed error.
pub fn col_index(meta: &TableMeta, name: &str) -> Result<usize> {
    meta.schema.index_of(name).ok_or_else(|| {
        MiniDbError::Catalog(format!("no such column {} in table {}", name, meta.name))
    })
}

#[allow(dead_code)]
fn _unused(_v: &Value) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, DataType, Schema};

    fn mk_schema() -> Schema {
        Schema::new(vec![
            Column {
                name: "a".into(),
                data_type: DataType::Int,
                primary_key: true,
                not_null: true,
                unique: true,
                default: None,
            },
            Column {
                name: "b".into(),
                data_type: DataType::Text,
                primary_key: false,
                not_null: false,
                unique: false,
                default: None,
            },
        ])
    }

    #[test]
    fn create_lookup_table() {
        let mut c = Catalog::new();
        c.tables.insert(
            "t".into(),
            TableMeta {
                name: "t".into(),
                schema: mk_schema(),
                rows: Default::default(),
            },
        );
        assert!(c.get_table("T").is_some());
        assert!(c.get_table("missing").is_none());
    }

    #[test]
    fn next_ids_increment() {
        let mut c = Catalog::new();
        let r1 = c.next_row_id();
        let r2 = c.next_row_id();
        assert_eq!(r2, r1 + 1);
        let t1 = c.allocate_txn();
        let t2 = c.allocate_txn();
        assert_eq!(t2, t1 + 1);
    }
}

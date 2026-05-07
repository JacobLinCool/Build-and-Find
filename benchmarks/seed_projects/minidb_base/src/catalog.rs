//! System catalog: metadata for tables and indexes, plus the in-memory
//! heap/index instances.
//!
//! The catalog is what `.schema`, `.tables`, and `.indexes` report from.
//! It also mediates DDL execution (CREATE / DROP TABLE / INDEX).

use crate::error::{MiniDbError, Result};
use crate::storage::{BTreeIndex, Heap, Index};
use crate::types::{Schema, Value};
use std::collections::BTreeMap;

/// Definition of a secondary index in the catalog.
#[derive(Debug, Clone)]
pub struct IndexDef {
    pub name: String,
    pub table: String,
    pub column: String,
    /// Zero-based position of `column` inside the table's schema.
    pub column_pos: usize,
    pub unique: bool,
}

/// The in-memory materialisation of a single table: its schema, its heap,
/// and a live map of the indexes that reference it.
pub struct TableInstance {
    pub schema: Schema,
    pub heap: Heap,
    pub indexes: BTreeMap<String, BTreeIndex>,
}

impl TableInstance {
    pub fn new(schema: Schema) -> Self {
        Self {
            schema,
            heap: Heap::new(),
            indexes: BTreeMap::new(),
        }
    }
}

/// The system catalog.
#[derive(Default)]
pub struct Catalog {
    tables: BTreeMap<String, TableInstance>,
    index_defs: BTreeMap<String, IndexDef>,
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a fresh table. Fails if a table with the same (case-insensitive)
    /// name already exists.
    pub fn create_table(&mut self, name: &str, schema: Schema) -> Result<()> {
        let key = name.to_ascii_lowercase();
        if self.tables.contains_key(&key) {
            return Err(MiniDbError::Catalog(format!(
                "table `{name}` already exists"
            )));
        }
        self.tables.insert(key, TableInstance::new(schema));
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        let key = name.to_ascii_lowercase();
        if self.tables.remove(&key).is_none() {
            return Err(MiniDbError::Catalog(format!("table `{name}` not found")));
        }
        self.index_defs
            .retain(|_, def| def.table.to_ascii_lowercase() != key);
        Ok(())
    }

    pub fn table(&self, name: &str) -> Result<&TableInstance> {
        let key = name.to_ascii_lowercase();
        self.tables
            .get(&key)
            .ok_or_else(|| MiniDbError::Catalog(format!("table `{name}` not found")))
    }

    pub fn table_mut(&mut self, name: &str) -> Result<&mut TableInstance> {
        let key = name.to_ascii_lowercase();
        self.tables
            .get_mut(&key)
            .ok_or_else(|| MiniDbError::Catalog(format!("table `{name}` not found")))
    }

    pub fn table_names(&self) -> Vec<String> {
        self.tables.keys().cloned().collect()
    }

    pub fn index_names(&self) -> Vec<String> {
        self.index_defs.keys().cloned().collect()
    }

    pub fn index_defs_for(&self, table: &str) -> Vec<IndexDef> {
        let tname = table.to_ascii_lowercase();
        self.index_defs
            .values()
            .filter(|d| d.table.to_ascii_lowercase() == tname)
            .cloned()
            .collect()
    }

    pub fn index_def(&self, name: &str) -> Option<&IndexDef> {
        self.index_defs.get(&name.to_ascii_lowercase())
    }

    /// Create a new secondary index and populate it from the current heap.
    pub fn create_index(
        &mut self,
        name: &str,
        table: &str,
        column: &str,
        unique: bool,
    ) -> Result<()> {
        let idx_key = name.to_ascii_lowercase();
        if self.index_defs.contains_key(&idx_key) {
            return Err(MiniDbError::Catalog(format!(
                "index `{name}` already exists"
            )));
        }
        let tbl = self.table_mut(table)?;
        let pos = tbl
            .schema
            .position(column)
            .ok_or_else(|| MiniDbError::Catalog(format!("column `{column}` not found")))?;

        let mut idx = BTreeIndex::new();
        for (rid, row) in tbl.heap.iter() {
            idx.insert(row[pos].clone(), rid)?;
        }
        tbl.indexes.insert(idx_key.clone(), idx);

        self.index_defs.insert(
            idx_key,
            IndexDef {
                name: name.to_string(),
                table: table.to_string(),
                column: column.to_string(),
                column_pos: pos,
                unique,
            },
        );
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<()> {
        let key = name.to_ascii_lowercase();
        let def = self
            .index_defs
            .remove(&key)
            .ok_or_else(|| MiniDbError::Catalog(format!("index `{name}` not found")))?;
        if let Ok(tbl) = self.table_mut(&def.table) {
            tbl.indexes.remove(&key);
        }
        Ok(())
    }

    /// Bookkeeping helper called by the insert executor to keep all
    /// indexes on `table` in sync with the heap.
    pub fn index_insert_row(&mut self, table: &str, row: &[Value], row_id: u64) -> Result<()> {
        let defs = self.index_defs_for(table);
        let tbl = self.table_mut(table)?;
        for def in defs {
            if let Some(idx) = tbl.indexes.get_mut(&def.name.to_ascii_lowercase()) {
                idx.insert(
                    row[def.column_pos].clone(),
                    crate::storage::RowId(row_id),
                )?;
            }
        }
        Ok(())
    }

    pub fn index_delete_row(&mut self, table: &str, row: &[Value], row_id: u64) -> Result<()> {
        let defs = self.index_defs_for(table);
        let tbl = self.table_mut(table)?;
        for def in defs {
            if let Some(idx) = tbl.indexes.get_mut(&def.name.to_ascii_lowercase()) {
                idx.remove(&row[def.column_pos], crate::storage::RowId(row_id))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, ColumnType};

    fn tiny_schema() -> Schema {
        Schema::new(vec![
            Column::new("id", ColumnType::Int),
            Column::new("name", ColumnType::Text),
        ])
    }

    #[test]
    fn create_and_drop_table() {
        let mut c = Catalog::new();
        c.create_table("Users", tiny_schema()).unwrap();
        assert!(c.create_table("users", tiny_schema()).is_err());
        c.drop_table("USERS").unwrap();
        assert!(c.table("users").is_err());
    }

    #[test]
    fn indexes_track_table_dropping() {
        let mut c = Catalog::new();
        c.create_table("t", tiny_schema()).unwrap();
        c.create_index("ix", "t", "id", false).unwrap();
        c.drop_table("t").unwrap();
        assert!(c.index_def("ix").is_none());
    }
}

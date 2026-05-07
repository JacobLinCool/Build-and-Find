use crate::error::{MiniDbError, Result};
use crate::parser::ast::Expr;
use crate::types::{Column, DataType, Schema, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Table metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableMeta {
    pub name: String,
    pub schema: Schema,
    pub check_constraints: Vec<SerializableExpr>,
    /// Root page of the heap file
    pub heap_root: u64,
    /// Next row id to assign
    pub next_row_id: u64,
}

/// A simplified serializable expression for CHECK constraints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SerializableExpr {
    Literal(Value),
    Column(String),
    BinOp(String, Box<SerializableExpr>, Box<SerializableExpr>),
    UnaryOp(String, Box<SerializableExpr>),
    IsNull(Box<SerializableExpr>, bool),
    FunctionCall(String, Vec<SerializableExpr>),
}

/// Index metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    pub unique: bool,
    /// Root page of the B-tree index
    pub btree_root: u64,
}

/// The catalog manages all table and index metadata
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub tables: HashMap<String, TableMeta>,
    pub indexes: HashMap<String, IndexMeta>,
    /// Map from table name to list of index names on that table
    pub table_indexes: HashMap<String, Vec<String>>,
}

impl Catalog {
    pub fn new() -> Self {
        Catalog::default()
    }

    pub fn create_table(&mut self, meta: TableMeta) -> Result<()> {
        let name_lower = meta.name.to_lowercase();
        if self.tables.contains_key(&name_lower) {
            return Err(MiniDbError::AlreadyExists(format!(
                "table '{}' already exists",
                meta.name
            )));
        }
        self.table_indexes.insert(name_lower.clone(), Vec::new());
        self.tables.insert(name_lower, meta);
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<TableMeta> {
        let name_lower = name.to_lowercase();
        let meta = self
            .tables
            .remove(&name_lower)
            .ok_or_else(|| MiniDbError::NotFound(format!("table '{}' not found", name)))?;

        // Remove indexes on this table
        if let Some(idx_names) = self.table_indexes.remove(&name_lower) {
            for idx_name in idx_names {
                self.indexes.remove(&idx_name.to_lowercase());
            }
        }

        Ok(meta)
    }

    pub fn get_table(&self, name: &str) -> Result<&TableMeta> {
        self.tables
            .get(&name.to_lowercase())
            .ok_or_else(|| MiniDbError::NotFound(format!("table '{}' not found", name)))
    }

    pub fn get_table_mut(&mut self, name: &str) -> Result<&mut TableMeta> {
        self.tables
            .get_mut(&name.to_lowercase())
            .ok_or_else(|| MiniDbError::NotFound(format!("table '{}' not found", name)))
    }

    pub fn create_index(&mut self, meta: IndexMeta) -> Result<()> {
        let idx_name_lower = meta.name.to_lowercase();
        if self.indexes.contains_key(&idx_name_lower) {
            return Err(MiniDbError::AlreadyExists(format!(
                "index '{}' already exists",
                meta.name
            )));
        }
        // Verify table exists
        let table_lower = meta.table.to_lowercase();
        if !self.tables.contains_key(&table_lower) {
            return Err(MiniDbError::NotFound(format!(
                "table '{}' not found",
                meta.table
            )));
        }
        // Verify columns exist
        let table = self.tables.get(&table_lower).unwrap();
        for col in &meta.columns {
            if table.schema.find_column(col).is_none() {
                return Err(MiniDbError::Schema(format!(
                    "column '{}' not found in table '{}'",
                    col, meta.table
                )));
            }
        }

        self.table_indexes
            .entry(table_lower)
            .or_default()
            .push(meta.name.clone());
        self.indexes.insert(idx_name_lower, meta);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<IndexMeta> {
        let name_lower = name.to_lowercase();
        let meta = self
            .indexes
            .remove(&name_lower)
            .ok_or_else(|| MiniDbError::NotFound(format!("index '{}' not found", name)))?;

        // Remove from table_indexes
        let table_lower = meta.table.to_lowercase();
        if let Some(idx_list) = self.table_indexes.get_mut(&table_lower) {
            idx_list.retain(|n| n.to_lowercase() != name_lower);
        }

        Ok(meta)
    }

    pub fn get_index(&self, name: &str) -> Result<&IndexMeta> {
        self.indexes
            .get(&name.to_lowercase())
            .ok_or_else(|| MiniDbError::NotFound(format!("index '{}' not found", name)))
    }

    pub fn table_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tables.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn index_names_for_table(&self, table: &str) -> Vec<String> {
        self.table_indexes
            .get(&table.to_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    pub fn all_index_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.indexes.keys().cloned().collect();
        names.sort();
        names
    }

    /// Find an index on the given table that covers the given column
    pub fn find_index_for_column(&self, table: &str, column: &str) -> Option<&IndexMeta> {
        let idx_names = self.table_indexes.get(&table.to_lowercase())?;
        for idx_name in idx_names {
            if let Some(idx) = self.indexes.get(&idx_name.to_lowercase()) {
                if idx.columns.len() == 1 && idx.columns[0].eq_ignore_ascii_case(column) {
                    return Some(idx);
                }
            }
        }
        None
    }

    pub fn serialize(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| MiniDbError::Serialization(e.to_string()))
    }

    pub fn deserialize(data: &[u8]) -> Result<Catalog> {
        serde_json::from_slice(data).map_err(|e| MiniDbError::Serialization(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, DataType, Schema};

    fn make_table(name: &str) -> TableMeta {
        TableMeta {
            name: name.to_string(),
            schema: Schema::new(vec![
                Column::new("id", DataType::Int),
                Column::new("name", DataType::Text),
            ]),
            check_constraints: Vec::new(),
            heap_root: 0,
            next_row_id: 1,
        }
    }

    #[test]
    fn test_create_and_get_table() {
        let mut cat = Catalog::new();
        cat.create_table(make_table("users")).unwrap();
        let t = cat.get_table("users").unwrap();
        assert_eq!(t.name, "users");
    }

    #[test]
    fn test_duplicate_table() {
        let mut cat = Catalog::new();
        cat.create_table(make_table("users")).unwrap();
        assert!(cat.create_table(make_table("users")).is_err());
    }

    #[test]
    fn test_drop_table() {
        let mut cat = Catalog::new();
        cat.create_table(make_table("users")).unwrap();
        cat.drop_table("users").unwrap();
        assert!(cat.get_table("users").is_err());
    }

    #[test]
    fn test_catalog_serialization() {
        let mut cat = Catalog::new();
        cat.create_table(make_table("users")).unwrap();
        let bytes = cat.serialize().unwrap();
        let cat2 = Catalog::deserialize(&bytes).unwrap();
        assert!(cat2.get_table("users").is_ok());
    }
}

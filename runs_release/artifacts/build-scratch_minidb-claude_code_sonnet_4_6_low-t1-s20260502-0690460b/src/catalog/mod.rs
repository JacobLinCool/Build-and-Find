use crate::error::{MiniDbError, Result};
use crate::types::{ColumnDef, DataType, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub primary_key: Option<usize>, // column index
    pub checks: Vec<String>,        // serialized check expressions (placeholder)
}

impl TableSchema {
    pub fn new(name: impl Into<String>, columns: Vec<ColumnDef>) -> Self {
        let primary_key = columns.iter().position(|c| c.primary_key);
        TableSchema {
            name: name.into(),
            columns,
            primary_key,
            checks: Vec::new(),
        }
    }

    pub fn col_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn col_count(&self) -> usize {
        self.columns.len()
    }

    /// Build a full row from named values, filling defaults / NULL for omitted
    pub fn build_row(&self, named: &[(String, Value)]) -> Result<Vec<Value>> {
        let mut row = vec![Value::Null; self.columns.len()];
        // Apply defaults first
        for (i, col) in self.columns.iter().enumerate() {
            if let Some(def) = &col.default {
                row[i] = def.clone();
            }
        }
        // Override with provided values
        for (name, val) in named {
            let idx = self
                .col_index(name)
                .ok_or_else(|| MiniDbError::ColumnNotFound(name.clone()))?;
            row[idx] = val.clone();
        }
        // Validate constraints
        for (i, col) in self.columns.iter().enumerate() {
            let v = &row[i];
            if col.not_null && v.is_null() {
                return Err(MiniDbError::Constraint(format!(
                    "column '{}' cannot be NULL",
                    col.name
                )));
            }
            if !v.is_null() {
                row[i] = v.coerce_to(&col.dtype)?;
            }
        }
        Ok(row)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    pub name: String,
    pub table: String,
    pub column: String,
    pub unique: bool,
    pub root_page: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub tables: HashMap<String, TableSchema>,
    pub indexes: HashMap<String, IndexMeta>,
    /// table name -> list of index names
    pub table_indexes: HashMap<String, Vec<String>>,
}

impl Catalog {
    pub fn new() -> Self {
        Catalog {
            tables: HashMap::new(),
            indexes: HashMap::new(),
            table_indexes: HashMap::new(),
        }
    }

    pub fn create_table(&mut self, schema: TableSchema) -> Result<()> {
        let name = schema.name.clone();
        if self.tables.contains_key(&name) {
            return Err(MiniDbError::TableExists(name));
        }
        self.tables.insert(name.clone(), schema);
        self.table_indexes.entry(name).or_default();
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        if self.tables.remove(name).is_none() {
            return Err(MiniDbError::TableNotFound(name.into()));
        }
        // Remove associated indexes
        if let Some(idx_names) = self.table_indexes.remove(name) {
            for idx in idx_names {
                self.indexes.remove(&idx);
            }
        }
        Ok(())
    }

    pub fn get_table(&self, name: &str) -> Result<&TableSchema> {
        self.tables
            .get(name)
            .ok_or_else(|| MiniDbError::TableNotFound(name.into()))
    }

    pub fn create_index(&mut self, meta: IndexMeta) -> Result<()> {
        let idx_name = meta.name.clone();
        if self.indexes.contains_key(&idx_name) {
            return Err(MiniDbError::IndexExists(idx_name));
        }
        let tbl = meta.table.clone();
        self.indexes.insert(idx_name.clone(), meta);
        self.table_indexes.entry(tbl).or_default().push(idx_name);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<()> {
        let meta = self
            .indexes
            .remove(name)
            .ok_or_else(|| MiniDbError::IndexNotFound(name.into()))?;
        if let Some(v) = self.table_indexes.get_mut(&meta.table) {
            v.retain(|n| n != name);
        }
        Ok(())
    }

    pub fn get_index(&self, name: &str) -> Result<&IndexMeta> {
        self.indexes
            .get(name)
            .ok_or_else(|| MiniDbError::IndexNotFound(name.into()))
    }

    pub fn indexes_for_table(&self, table: &str) -> Vec<&IndexMeta> {
        self.table_indexes
            .get(table)
            .map(|names| names.iter().filter_map(|n| self.indexes.get(n)).collect())
            .unwrap_or_default()
    }

    pub fn serialize(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| MiniDbError::Serialization(e.to_string()))
    }

    pub fn deserialize(data: &[u8]) -> Result<Self> {
        serde_json::from_slice(data).map_err(|e| MiniDbError::Serialization(e.to_string()))
    }
}

impl Default for Catalog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_col(name: &str, dtype: DataType) -> ColumnDef {
        ColumnDef::new(name, dtype)
    }

    #[test]
    fn test_create_and_get_table() {
        let mut cat = Catalog::new();
        let schema = TableSchema::new(
            "users",
            vec![
                make_col("id", DataType::Int),
                make_col("name", DataType::Text),
            ],
        );
        cat.create_table(schema).unwrap();
        let t = cat.get_table("users").unwrap();
        assert_eq!(t.col_count(), 2);
    }

    #[test]
    fn test_duplicate_table_error() {
        let mut cat = Catalog::new();
        let schema = TableSchema::new("t", vec![make_col("x", DataType::Int)]);
        cat.create_table(schema.clone()).unwrap();
        assert!(cat.create_table(schema).is_err());
    }

    #[test]
    fn test_drop_table() {
        let mut cat = Catalog::new();
        cat.create_table(TableSchema::new("t", vec![make_col("x", DataType::Int)]))
            .unwrap();
        cat.drop_table("t").unwrap();
        assert!(cat.get_table("t").is_err());
    }

    #[test]
    fn test_catalog_roundtrip() {
        let mut cat = Catalog::new();
        cat.create_table(TableSchema::new("t", vec![make_col("x", DataType::Int)]))
            .unwrap();
        let bytes = cat.serialize().unwrap();
        let cat2 = Catalog::deserialize(&bytes).unwrap();
        assert!(cat2.get_table("t").is_ok());
    }
}

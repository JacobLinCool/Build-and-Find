use crate::error::{MiniDbError, Result};
use crate::storage::btree::{BTreeIndex, IndexKey};
use crate::storage::RowId;
use crate::types::{ColumnType, Value};
use log::debug;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CatalogData {
    tables: Vec<TableSchema>,
    indexes: Vec<IndexSchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnSchema>,
    pub check_constraints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnSchema {
    pub name: String,
    pub col_type: ColumnType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSchema {
    pub name: String,
    pub table: String,
    pub column: String,
}

pub struct Catalog {
    tables: HashMap<String, TableSchema>,
    indexes: HashMap<String, IndexSchema>,
    index_data: HashMap<String, BTreeIndex>,
}

impl Catalog {
    pub fn new() -> Self {
        Self {
            tables: HashMap::new(),
            indexes: HashMap::new(),
            index_data: HashMap::new(),
        }
    }

    pub fn create_table(&mut self, schema: TableSchema) -> Result<()> {
        if self.tables.contains_key(&schema.name) {
            return Err(MiniDbError::Runtime(format!(
                "Table '{}' already exists",
                schema.name
            )));
        }
        debug!("Created table '{}'", schema.name);
        self.tables.insert(schema.name.clone(), schema);
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        if self.tables.remove(name).is_none() {
            return Err(MiniDbError::NotFound(format!("Table '{}' not found", name)));
        }
        // Remove associated indexes
        let indexes_to_remove: Vec<String> = self
            .indexes
            .iter()
            .filter(|(_, idx)| idx.table == name)
            .map(|(name, _)| name.clone())
            .collect();
        for idx_name in indexes_to_remove {
            self.indexes.remove(&idx_name);
            self.index_data.remove(&idx_name);
        }
        debug!("Dropped table '{}'", name);
        Ok(())
    }

    pub fn get_table(&self, name: &str) -> Result<&TableSchema> {
        self.tables
            .get(name)
            .ok_or_else(|| MiniDbError::NotFound(format!("Table '{}' not found", name)))
    }

    pub fn get_table_mut(&mut self, name: &str) -> Result<&mut TableSchema> {
        self.tables
            .get_mut(name)
            .ok_or_else(|| MiniDbError::NotFound(format!("Table '{}' not found", name)))
    }

    pub fn table_exists(&self, name: &str) -> bool {
        self.tables.contains_key(name)
    }

    pub fn all_table_names(&self) -> Vec<&str> {
        self.tables.keys().map(|s| s.as_str()).collect()
    }

    pub fn create_index(&mut self, schema: IndexSchema) -> Result<()> {
        if self.indexes.contains_key(&schema.name) {
            return Err(MiniDbError::Runtime(format!(
                "Index '{}' already exists",
                schema.name
            )));
        }
        if !self.tables.contains_key(&schema.table) {
            return Err(MiniDbError::NotFound(format!(
                "Table '{}' not found",
                schema.table
            )));
        }
        let table = self.tables.get(&schema.table).unwrap();
        if !table.columns.iter().any(|c| c.name == schema.column) {
            return Err(MiniDbError::NotFound(format!(
                "Column '{}' not found in table '{}'",
                schema.column, schema.table
            )));
        }
        let idx = BTreeIndex::new(&schema.name, &schema.table, &schema.column);
        self.index_data.insert(schema.name.clone(), idx);
        self.indexes.insert(schema.name.clone(), schema);
        debug!("Created index '{}'", self.indexes.len());
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<()> {
        if self.indexes.remove(name).is_none() {
            return Err(MiniDbError::NotFound(format!("Index '{}' not found", name)));
        }
        self.index_data.remove(name);
        debug!("Dropped index '{}'", name);
        Ok(())
    }

    pub fn get_index(&self, name: &str) -> Result<&IndexSchema> {
        self.indexes
            .get(name)
            .ok_or_else(|| MiniDbError::NotFound(format!("Index '{}' not found", name)))
    }

    pub fn get_index_data(&self, name: &str) -> Result<&BTreeIndex> {
        self.index_data
            .get(name)
            .ok_or_else(|| MiniDbError::NotFound(format!("Index data '{}' not found", name)))
    }

    pub fn get_index_data_mut(&mut self, name: &str) -> Result<&mut BTreeIndex> {
        self.index_data
            .get_mut(name)
            .ok_or_else(|| MiniDbError::NotFound(format!("Index data '{}' not found", name)))
    }

    pub fn all_index_names(&self) -> Vec<&str> {
        self.indexes.keys().map(|s| s.as_str()).collect()
    }

    pub fn find_index_for_column(&self, table: &str, column: &str) -> Option<&str> {
        self.indexes
            .values()
            .find(|idx| idx.table == table && idx.column == column)
            .map(|idx| idx.name.as_str())
    }

    pub fn find_all_indexes_for_table(&self, table: &str) -> Vec<&str> {
        self.indexes
            .values()
            .filter(|idx| idx.table == table)
            .map(|idx| idx.name.as_str())
            .collect()
    }

    pub fn column_index(&self, table: &str, column: &str) -> Result<usize> {
        let schema = self.get_table(table)?;
        schema
            .columns
            .iter()
            .position(|c| c.name == column)
            .ok_or_else(|| {
                MiniDbError::NotFound(format!(
                    "Column '{}' not found in table '{}'",
                    column, table
                ))
            })
    }

    pub fn column_type(&self, table: &str, column: &str) -> Result<ColumnType> {
        let schema = self.get_table(table)?;
        schema
            .columns
            .iter()
            .find(|c| c.name == column)
            .map(|c| c.col_type.clone())
            .ok_or_else(|| {
                MiniDbError::NotFound(format!(
                    "Column '{}' not found in table '{}'",
                    column, table
                ))
            })
    }

    pub fn add_index_entry(
        &mut self,
        index_name: &str,
        key: IndexKey,
        row_id: RowId,
    ) -> Result<()> {
        if let Some(idx) = self.index_data.get_mut(index_name) {
            idx.insert(key, row_id);
        }
        Ok(())
    }

    pub fn remove_index_entry(
        &mut self,
        index_name: &str,
        key: &IndexKey,
        row_id: RowId,
    ) -> Result<()> {
        if let Some(idx) = self.index_data.get_mut(index_name) {
            idx.remove(key, row_id);
        }
        Ok(())
    }

    pub fn schema_table(&self) -> Vec<(&str, &str)> {
        self.tables
            .values()
            .map(|t| (t.name.as_str(), format_columns(&t.columns)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(name, _)| (name, ""))
            .collect()
    }

    pub fn schema_sql(&self, table_name: &str) -> Result<String> {
        let table = self.get_table(table_name)?;
        let mut sql = format!("CREATE TABLE {} (\n", table.name);
        let mut parts = Vec::new();
        for col in &table.columns {
            let mut part = format!("  {} {}", col.name, col.col_type.name());
            if col.primary_key {
                part += " PRIMARY KEY";
            }
            if col.not_null && !col.primary_key {
                part += " NOT NULL";
            }
            if col.unique {
                part += " UNIQUE";
            }
            if let Some(ref default) = col.default {
                part += &format!(" DEFAULT {}", default);
            }
            parts.push(part);
        }
        sql += &parts.join(",\n");
        sql += "\n)";
        Ok(sql)
    }
}

fn format_columns(columns: &[ColumnSchema]) -> String {
    columns
        .iter()
        .map(|c| format!("{}: {}", c.name, c.col_type.name()))
        .collect::<Vec<_>>()
        .join(", ")
}

impl Default for Catalog {
    fn default() -> Self {
        Self::new()
    }
}

impl Catalog {
    pub fn save(&self, path: &str) -> Result<()> {
        let data = CatalogData {
            tables: self.tables.values().cloned().collect(),
            indexes: self.indexes.values().cloned().collect(),
        };
        let json =
            serde_json::to_string_pretty(&data).map_err(|e| MiniDbError::Storage(e.to_string()))?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load(path: &str) -> Result<Self> {
        let json = fs::read_to_string(path)?;
        let data: CatalogData =
            serde_json::from_str(&json).map_err(|e| MiniDbError::Storage(e.to_string()))?;
        let mut tables = HashMap::new();
        for t in data.tables {
            tables.insert(t.name.clone(), t);
        }
        let mut indexes = HashMap::new();
        for idx in data.indexes {
            indexes.insert(idx.name.clone(), idx);
        }
        Ok(Self {
            tables,
            indexes,
            index_data: HashMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_create_table() {
        let mut cat = Catalog::new();
        let schema = TableSchema {
            name: "users".into(),
            columns: vec![
                ColumnSchema {
                    name: "id".into(),
                    col_type: ColumnType::Int,
                    primary_key: true,
                    not_null: true,
                    unique: false,
                    default: None,
                },
                ColumnSchema {
                    name: "name".into(),
                    col_type: ColumnType::Text,
                    primary_key: false,
                    not_null: false,
                    unique: false,
                    default: None,
                },
            ],
            check_constraints: vec![],
        };
        cat.create_table(schema).unwrap();
        assert!(cat.table_exists("users"));
        assert_eq!(cat.all_table_names().len(), 1);
    }

    #[test]
    fn test_catalog_drop_table() {
        let mut cat = Catalog::new();
        let schema = TableSchema {
            name: "t".into(),
            columns: vec![ColumnSchema {
                name: "id".into(),
                col_type: ColumnType::Int,
                primary_key: true,
                not_null: true,
                unique: false,
                default: None,
            }],
            check_constraints: vec![],
        };
        cat.create_table(schema).unwrap();
        cat.drop_table("t").unwrap();
        assert!(!cat.table_exists("t"));
    }

    #[test]
    fn test_catalog_create_index() {
        let mut cat = Catalog::new();
        let schema = TableSchema {
            name: "users".into(),
            columns: vec![ColumnSchema {
                name: "id".into(),
                col_type: ColumnType::Int,
                primary_key: true,
                not_null: true,
                unique: false,
                default: None,
            }],
            check_constraints: vec![],
        };
        cat.create_table(schema).unwrap();
        let idx = IndexSchema {
            name: "idx_id".into(),
            table: "users".into(),
            column: "id".into(),
        };
        cat.create_index(idx).unwrap();
        assert_eq!(cat.all_index_names(), vec!["idx_id"]);
    }
}

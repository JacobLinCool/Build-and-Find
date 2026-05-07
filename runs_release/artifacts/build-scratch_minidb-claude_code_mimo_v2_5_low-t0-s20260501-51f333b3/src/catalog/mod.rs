use crate::error::{MiniDbError, Result};
use crate::parser::ast::ColumnDef;
use crate::types::{Column, ColumnType, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<Column>,
    pub root_page: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexInfo {
    pub name: String,
    pub table: String,
    pub column: String,
    pub root_page: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CatalogData {
    tables: Vec<TableSchema>,
    indexes: Vec<IndexInfo>,
    next_page_id: u64,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    tables: HashMap<String, TableSchema>,
    indexes: HashMap<String, IndexInfo>,
    next_page_id: u64,
}

impl Catalog {
    pub fn new() -> Self {
        Catalog {
            tables: HashMap::new(),
            indexes: HashMap::new(),
            next_page_id: 1,
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let data = CatalogData {
            tables: self.tables.values().cloned().collect(),
            indexes: self.indexes.values().cloned().collect(),
            next_page_id: self.next_page_id,
        };
        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| MiniDbError::StorageError(format!("catalog serialize: {}", e)))?;
        fs::write(path, json)
            .map_err(|e| MiniDbError::StorageError(format!("catalog write: {}", e)))?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Catalog::new());
        }
        let json = fs::read_to_string(path)
            .map_err(|e| MiniDbError::StorageError(format!("catalog read: {}", e)))?;
        let data: CatalogData = serde_json::from_str(&json)
            .map_err(|e| MiniDbError::StorageError(format!("catalog deserialize: {}", e)))?;
        let mut tables = HashMap::new();
        for t in data.tables {
            tables.insert(t.name.clone(), t);
        }
        let mut indexes = HashMap::new();
        for idx in data.indexes {
            indexes.insert(idx.name.clone(), idx);
        }
        Ok(Catalog {
            tables,
            indexes,
            next_page_id: data.next_page_id,
        })
    }

    pub fn next_page_id(&mut self) -> u64 {
        let id = self.next_page_id;
        self.next_page_id += 1;
        id
    }

    pub fn create_table(&mut self, name: String, col_defs: Vec<ColumnDef>) -> Result<()> {
        if self.tables.contains_key(&name) {
            return Err(MiniDbError::CatalogError(format!(
                "table '{}' already exists",
                name
            )));
        }

        let root_page = self.next_page_id();

        let columns: Vec<Column> = col_defs
            .into_iter()
            .map(|cd| Column {
                name: cd.name,
                col_type: cd.col_type,
                nullable: !cd.not_null && !cd.primary_key,
                primary_key: cd.primary_key,
                unique: cd.unique || cd.primary_key,
                default: cd.default.map(|l| l.to_value()),
                check: cd.check,
            })
            .collect();

        self.tables.insert(
            name.clone(),
            TableSchema {
                name,
                columns,
                root_page,
            },
        );

        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        if self.tables.remove(name).is_none() {
            return Err(MiniDbError::CatalogError(format!(
                "table '{}' does not exist",
                name
            )));
        }
        self.indexes.retain(|_, idx| idx.table != name);
        Ok(())
    }

    pub fn get_table(&self, name: &str) -> Result<&TableSchema> {
        self.tables
            .get(name)
            .ok_or_else(|| MiniDbError::CatalogError(format!("table '{}' does not exist", name)))
    }

    pub fn get_table_mut(&mut self, name: &str) -> Result<&mut TableSchema> {
        self.tables
            .get_mut(name)
            .ok_or_else(|| MiniDbError::CatalogError(format!("table '{}' does not exist", name)))
    }

    pub fn table_exists(&self, name: &str) -> bool {
        self.tables.contains_key(name)
    }

    pub fn all_table_names(&self) -> Vec<&str> {
        self.tables.keys().map(|s| s.as_str()).collect()
    }

    pub fn create_index(&mut self, name: String, table: String, column: String) -> Result<()> {
        if self.indexes.contains_key(&name) {
            return Err(MiniDbError::CatalogError(format!(
                "index '{}' already exists",
                name
            )));
        }
        if !self.tables.contains_key(&table) {
            return Err(MiniDbError::CatalogError(format!(
                "table '{}' does not exist",
                table
            )));
        }

        let root_page = self.next_page_id();

        self.indexes.insert(
            name.clone(),
            IndexInfo {
                name,
                table,
                column,
                root_page,
            },
        );

        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<()> {
        if self.indexes.remove(name).is_none() {
            return Err(MiniDbError::CatalogError(format!(
                "index '{}' does not exist",
                name
            )));
        }
        Ok(())
    }

    pub fn get_index(&self, name: &str) -> Result<&IndexInfo> {
        self.indexes
            .get(name)
            .ok_or_else(|| MiniDbError::CatalogError(format!("index '{}' does not exist", name)))
    }

    pub fn find_index_for_column(&self, table: &str, column: &str) -> Option<&IndexInfo> {
        self.indexes
            .values()
            .find(|idx| idx.table == table && idx.column == column)
    }

    pub fn all_index_names(&self) -> Vec<&str> {
        self.indexes.keys().map(|s| s.as_str()).collect()
    }

    pub fn column_index(&self, table: &str, column: &str) -> Result<usize> {
        let schema = self.get_table(table)?;
        schema
            .columns
            .iter()
            .position(|c| c.name == column)
            .ok_or_else(|| {
                MiniDbError::CatalogError(format!(
                    "column '{}' not found in table '{}'",
                    column, table
                ))
            })
    }

    pub fn default_values(&self, table: &str) -> Result<Vec<Value>> {
        let schema = self.get_table(table)?;
        Ok(schema
            .columns
            .iter()
            .map(|c| {
                c.default.clone().unwrap_or_else(
                    || {
                        if c.nullable {
                            Value::Null
                        } else {
                            Value::Null
                        }
                    },
                )
            })
            .collect())
    }

    pub fn validate_row(&self, table: &str, values: &[Value]) -> Result<()> {
        let schema = self.get_table(table)?;

        if values.len() != schema.columns.len() {
            return Err(MiniDbError::SqlError(format!(
                "expected {} values, got {}",
                schema.columns.len(),
                values.len()
            )));
        }

        for (col, val) in schema.columns.iter().zip(values.iter()) {
            if val.is_null() && !col.nullable {
                return Err(MiniDbError::SqlError(format!(
                    "NOT NULL constraint failed for column '{}'",
                    col.name
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SqlLiteral;

    #[test]
    fn test_create_and_drop_table() {
        let mut catalog = Catalog::new();
        let cols = vec![
            ColumnDef {
                name: "id".to_string(),
                col_type: ColumnType::Int,
                primary_key: true,
                not_null: true,
                unique: false,
                default: None,
                check: None,
            },
            ColumnDef {
                name: "name".to_string(),
                col_type: ColumnType::Text,
                primary_key: false,
                not_null: false,
                unique: false,
                default: None,
                check: None,
            },
        ];
        catalog.create_table("users".to_string(), cols).unwrap();
        assert!(catalog.table_exists("users"));
        catalog.drop_table("users").unwrap();
        assert!(!catalog.table_exists("users"));
    }

    #[test]
    fn test_duplicate_table_error() {
        let mut catalog = Catalog::new();
        let cols = vec![ColumnDef {
            name: "id".to_string(),
            col_type: ColumnType::Int,
            primary_key: true,
            not_null: true,
            unique: false,
            default: None,
            check: None,
        }];
        catalog.create_table("t".to_string(), cols.clone()).unwrap();
        assert!(catalog.create_table("t".to_string(), cols).is_err());
    }

    #[test]
    fn test_create_index() {
        let mut catalog = Catalog::new();
        let cols = vec![ColumnDef {
            name: "id".to_string(),
            col_type: ColumnType::Int,
            primary_key: true,
            not_null: true,
            unique: false,
            default: None,
            check: None,
        }];
        catalog.create_table("t".to_string(), cols).unwrap();
        catalog
            .create_index("idx".to_string(), "t".to_string(), "id".to_string())
            .unwrap();
        assert!(catalog.find_index_for_column("t", "id").is_some());
    }

    #[test]
    fn test_validate_row() {
        let mut catalog = Catalog::new();
        let cols = vec![
            ColumnDef {
                name: "id".to_string(),
                col_type: ColumnType::Int,
                primary_key: true,
                not_null: true,
                unique: false,
                default: None,
                check: None,
            },
            ColumnDef {
                name: "name".to_string(),
                col_type: ColumnType::Text,
                primary_key: false,
                not_null: true,
                unique: false,
                default: None,
                check: None,
            },
        ];
        catalog.create_table("t".to_string(), cols).unwrap();
        assert!(catalog
            .validate_row("t", &[Value::Int(1), Value::Text("hi".to_string())])
            .is_ok());
        assert!(catalog
            .validate_row("t", &[Value::Int(1), Value::Null])
            .is_err());
    }
}

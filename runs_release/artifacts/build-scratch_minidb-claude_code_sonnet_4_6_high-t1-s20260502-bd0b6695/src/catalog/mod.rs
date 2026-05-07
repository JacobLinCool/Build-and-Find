use crate::error::{MiniDbError, Result};
use crate::storage::page::PageId;
use crate::types::{ColumnDef, DataType, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Table schema stored in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub primary_key: Option<Vec<String>>,
    pub check_constraints: Vec<String>, // serialized check expr descriptions
    pub heap_first_page: u64,
    pub heap_last_page: u64,
    pub next_row_seq: u64, // not used for row_id anymore (RowId from page/slot)
}

impl TableSchema {
    pub fn col_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn col_types(&self) -> Vec<DataType> {
        self.columns.iter().map(|c| c.data_type.clone()).collect()
    }

    pub fn to_schema(&self) -> crate::types::Schema {
        crate::types::Schema::new(
            self.columns
                .iter()
                .map(|c| (c.name.clone(), c.data_type.clone()))
                .collect(),
        )
    }
}

/// Index definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDef {
    pub name: String,
    pub table: String,
    pub column: String,
    pub unique: bool,
    pub root_page: u64, // PageId(root_page) is the BTree root
}

/// Check constraint stored in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckConstraint {
    pub table: String,
    pub expr_source: String,
}

/// In-memory catalog backed by a JSON file.
#[derive(Debug, Serialize, Deserialize)]
pub struct Catalog {
    pub tables: HashMap<String, TableSchema>,
    pub indexes: HashMap<String, IndexDef>,
    // table_name -> [index_names]
    pub table_indexes: HashMap<String, Vec<String>>,
    #[serde(skip)]
    catalog_path: Option<std::path::PathBuf>,
}

impl Catalog {
    pub fn new() -> Self {
        Catalog {
            tables: HashMap::new(),
            indexes: HashMap::new(),
            table_indexes: HashMap::new(),
            catalog_path: None,
        }
    }

    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            if content.trim().is_empty() {
                let mut catalog = Catalog::new();
                catalog.catalog_path = Some(path.to_path_buf());
                return Ok(catalog);
            }
            let mut catalog: Catalog = serde_json::from_str(&content)?;
            catalog.catalog_path = Some(path.to_path_buf());
            Ok(catalog)
        } else {
            let mut catalog = Catalog::new();
            catalog.catalog_path = Some(path.to_path_buf());
            Ok(catalog)
        }
    }

    pub fn save(&self) -> Result<()> {
        if let Some(path) = &self.catalog_path {
            let content = serde_json::to_string_pretty(self)?;
            std::fs::write(path, content)?;
        }
        Ok(())
    }

    pub fn create_table(&mut self, schema: TableSchema) -> Result<()> {
        let name = schema.name.to_lowercase();
        if self.tables.contains_key(&name) {
            return Err(MiniDbError::TableExists(name));
        }
        self.table_indexes.insert(name.clone(), Vec::new());
        self.tables.insert(name, schema);
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        let name = name.to_lowercase();
        if self.tables.remove(&name).is_none() {
            return Err(MiniDbError::NoSuchTable(name));
        }
        // Remove all indexes for this table
        if let Some(idxs) = self.table_indexes.remove(&name) {
            for idx_name in idxs {
                self.indexes.remove(&idx_name);
            }
        }
        Ok(())
    }

    pub fn get_table(&self, name: &str) -> Option<&TableSchema> {
        self.tables.get(&name.to_lowercase())
    }

    pub fn get_table_mut(&mut self, name: &str) -> Option<&mut TableSchema> {
        self.tables.get_mut(&name.to_lowercase())
    }

    pub fn create_index(&mut self, def: IndexDef) -> Result<()> {
        let idx_name = def.name.to_lowercase();
        let table_name = def.table.to_lowercase();

        if self.indexes.contains_key(&idx_name) {
            return Err(MiniDbError::IndexExists(idx_name));
        }
        if !self.tables.contains_key(&table_name) {
            return Err(MiniDbError::NoSuchTable(table_name));
        }
        self.table_indexes
            .entry(table_name)
            .or_default()
            .push(idx_name.clone());
        self.indexes.insert(idx_name, def);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<()> {
        let name = name.to_lowercase();
        let def = self
            .indexes
            .remove(&name)
            .ok_or_else(|| MiniDbError::NoSuchIndex(name.clone()))?;
        let table_name = def.table.to_lowercase();
        if let Some(idxs) = self.table_indexes.get_mut(&table_name) {
            idxs.retain(|i| i != &name);
        }
        Ok(())
    }

    pub fn get_index(&self, name: &str) -> Option<&IndexDef> {
        self.indexes.get(&name.to_lowercase())
    }

    pub fn get_index_mut(&mut self, name: &str) -> Option<&mut IndexDef> {
        self.indexes.get_mut(&name.to_lowercase())
    }

    pub fn indexes_for_table(&self, table: &str) -> Vec<&IndexDef> {
        let table = table.to_lowercase();
        match self.table_indexes.get(&table) {
            None => vec![],
            Some(names) => names.iter().filter_map(|n| self.indexes.get(n)).collect(),
        }
    }

    /// Find an index on a specific column for a table.
    pub fn find_index(&self, table: &str, col: &str) -> Option<&IndexDef> {
        let table = table.to_lowercase();
        let col = col.to_lowercase();
        self.table_indexes
            .get(&table)?
            .iter()
            .filter_map(|n| self.indexes.get(n))
            .find(|idx| idx.column.to_lowercase() == col)
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
    use crate::types::DataType;
    use tempfile::NamedTempFile;

    #[test]
    fn test_catalog_create_drop_table() {
        let mut cat = Catalog::new();
        let schema = TableSchema {
            name: "users".into(),
            columns: vec![ColumnDef {
                name: "id".into(),
                data_type: DataType::Int,
                not_null: true,
                primary_key: true,
                unique: true,
                default: None,
            }],
            primary_key: Some(vec!["id".into()]),
            check_constraints: vec![],
            heap_first_page: 0,
            heap_last_page: 0,
            next_row_seq: 0,
        };
        cat.create_table(schema).unwrap();
        assert!(cat.get_table("users").is_some());
        assert!(cat
            .create_table(TableSchema {
                name: "users".into(),
                columns: vec![],
                primary_key: None,
                check_constraints: vec![],
                heap_first_page: 0,
                heap_last_page: 0,
                next_row_seq: 0,
            })
            .is_err()); // duplicate
        cat.drop_table("users").unwrap();
        assert!(cat.get_table("users").is_none());
    }

    #[test]
    fn test_catalog_serialize() {
        let tmp = NamedTempFile::new().unwrap();
        let mut cat = Catalog::load_or_create(tmp.path()).unwrap();
        cat.create_table(TableSchema {
            name: "t".into(),
            columns: vec![],
            primary_key: None,
            check_constraints: vec![],
            heap_first_page: 0,
            heap_last_page: 0,
            next_row_seq: 0,
        })
        .unwrap();
        cat.save().unwrap();

        let cat2 = Catalog::load_or_create(tmp.path()).unwrap();
        assert!(cat2.get_table("t").is_some());
    }
}

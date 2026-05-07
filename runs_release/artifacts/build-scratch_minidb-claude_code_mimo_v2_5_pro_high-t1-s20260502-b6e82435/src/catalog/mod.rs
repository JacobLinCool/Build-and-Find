use crate::error::{MiniDbError, Result};
use crate::types::{IndexDef, Schema, TableDef};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub tables: HashMap<String, TableDef>,
    pub indexes: HashMap<String, IndexDef>,
    pub table_next_row_id: HashMap<String, u64>,
    pub table_root_pages: HashMap<String, u64>,
    pub index_root_pages: HashMap<String, u64>,
}

impl Catalog {
    pub fn new() -> Self {
        Self {
            tables: HashMap::new(),
            indexes: HashMap::new(),
            table_next_row_id: HashMap::new(),
            table_root_pages: HashMap::new(),
            index_root_pages: HashMap::new(),
        }
    }

    pub fn create_table(&mut self, table: TableDef, root_page: u64) -> Result<()> {
        let name = table.name.clone();
        if self.tables.contains_key(&name) {
            return Err(MiniDbError::Catalog(format!(
                "table '{}' already exists",
                name
            )));
        }
        self.tables.insert(name.clone(), table);
        self.table_next_row_id.insert(name.clone(), 1);
        self.table_root_pages.insert(name, root_page);
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<TableDef> {
        let table = self
            .tables
            .remove(name)
            .ok_or_else(|| MiniDbError::Catalog(format!("table '{}' does not exist", name)))?;

        // Remove associated indexes
        let idx_names: Vec<String> = self
            .indexes
            .iter()
            .filter(|(_, idx)| idx.table_name == name)
            .map(|(n, _)| n.clone())
            .collect();
        for idx_name in idx_names {
            self.indexes.remove(&idx_name);
            self.index_root_pages.remove(&idx_name);
        }

        self.table_next_row_id.remove(name);
        self.table_root_pages.remove(name);
        Ok(table)
    }

    pub fn get_table(&self, name: &str) -> Result<&TableDef> {
        self.tables
            .get(name)
            .ok_or_else(|| MiniDbError::Catalog(format!("table '{}' does not exist", name)))
    }

    pub fn get_schema(&self, name: &str) -> Result<Schema> {
        Ok(self.get_table(name)?.schema())
    }

    pub fn next_row_id(&mut self, table: &str) -> u64 {
        let id = self.table_next_row_id.get(table).copied().unwrap_or(1);
        self.table_next_row_id.insert(table.to_string(), id + 1);
        id
    }

    pub fn create_index(&mut self, index: IndexDef, root_page: u64) -> Result<()> {
        let name = index.name.clone();
        if self.indexes.contains_key(&name) {
            return Err(MiniDbError::Catalog(format!(
                "index '{}' already exists",
                name
            )));
        }
        // Verify table exists
        self.get_table(&index.table_name)?;
        self.indexes.insert(name.clone(), index);
        self.index_root_pages.insert(name, root_page);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<IndexDef> {
        let idx = self
            .indexes
            .remove(name)
            .ok_or_else(|| MiniDbError::Catalog(format!("index '{}' does not exist", name)))?;
        self.index_root_pages.remove(name);
        Ok(idx)
    }

    pub fn get_index(&self, name: &str) -> Option<&IndexDef> {
        self.indexes.get(name)
    }

    pub fn indexes_for_table(&self, table: &str) -> Vec<&IndexDef> {
        self.indexes
            .values()
            .filter(|idx| idx.table_name == table)
            .collect()
    }

    pub fn table_names(&self) -> Vec<&str> {
        self.tables.keys().map(|s| s.as_str()).collect()
    }

    pub fn index_names(&self) -> Vec<&str> {
        self.indexes.keys().map(|s| s.as_str()).collect()
    }

    pub fn table_root_page(&self, name: &str) -> Option<u64> {
        self.table_root_pages.get(name).copied()
    }

    pub fn index_root_page(&self, name: &str) -> Option<u64> {
        self.index_root_pages.get(name).copied()
    }

    pub fn serialize(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn deserialize(data: &[u8]) -> Result<Self> {
        serde_json::from_slice(data)
            .map_err(|e| MiniDbError::Catalog(format!("failed to deserialize catalog: {}", e)))
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
    use crate::types::{ColumnConstraint, ColumnDef, ColumnType};

    fn sample_table(name: &str) -> TableDef {
        TableDef {
            name: name.to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    col_type: ColumnType::Int,
                    constraints: vec![ColumnConstraint::PrimaryKey],
                },
                ColumnDef {
                    name: "name".into(),
                    col_type: ColumnType::Text,
                    constraints: vec![],
                },
            ],
            check_constraints: vec![],
        }
    }

    #[test]
    fn test_create_and_get_table() {
        let mut catalog = Catalog::new();
        catalog.create_table(sample_table("users"), 1).unwrap();
        let table = catalog.get_table("users").unwrap();
        assert_eq!(table.name, "users");
    }

    #[test]
    fn test_drop_table() {
        let mut catalog = Catalog::new();
        catalog.create_table(sample_table("users"), 1).unwrap();
        catalog.drop_table("users").unwrap();
        assert!(catalog.get_table("users").is_err());
    }

    #[test]
    fn test_duplicate_table() {
        let mut catalog = Catalog::new();
        catalog.create_table(sample_table("users"), 1).unwrap();
        assert!(catalog.create_table(sample_table("users"), 2).is_err());
    }

    #[test]
    fn test_index_operations() {
        let mut catalog = Catalog::new();
        catalog.create_table(sample_table("users"), 1).unwrap();

        let idx = IndexDef {
            name: "idx_name".into(),
            table_name: "users".into(),
            column_name: "name".into(),
        };
        catalog.create_index(idx, 2).unwrap();

        assert!(catalog.get_index("idx_name").is_some());
        assert_eq!(catalog.indexes_for_table("users").len(), 1);

        catalog.drop_index("idx_name").unwrap();
        assert!(catalog.get_index("idx_name").is_none());
    }

    #[test]
    fn test_row_id_generation() {
        let mut catalog = Catalog::new();
        assert_eq!(catalog.next_row_id("users"), 1);
        assert_eq!(catalog.next_row_id("users"), 2);
        assert_eq!(catalog.next_row_id("users"), 3);
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut catalog = Catalog::new();
        catalog.create_table(sample_table("users"), 1).unwrap();

        let data = catalog.serialize();
        let restored = Catalog::deserialize(&data).unwrap();
        assert!(restored.get_table("users").is_ok());
    }
}

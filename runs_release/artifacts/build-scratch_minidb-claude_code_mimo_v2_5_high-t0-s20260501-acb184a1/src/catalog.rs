use std::collections::HashMap;

use crate::error::{MiniDbError, Result};
use crate::types::{ColumnDef, IndexDef, TableSchema};

/// Catalog manages table and index metadata.
#[derive(Debug)]
pub struct Catalog {
    tables: HashMap<String, TableSchema>,
    indexes: HashMap<String, IndexDef>,
}

impl Catalog {
    pub fn new() -> Self {
        Catalog {
            tables: HashMap::new(),
            indexes: HashMap::new(),
        }
    }

    pub fn create_table(&mut self, schema: TableSchema) -> Result<()> {
        if self.tables.contains_key(&schema.name) {
            return Err(MiniDbError::TableAlreadyExists(schema.name.clone()));
        }
        self.tables.insert(schema.name.clone(), schema);
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        if self.tables.remove(name).is_none() {
            return Err(MiniDbError::TableNotFound(name.to_string()));
        }
        // Remove indexes on this table
        self.indexes.retain(|_, idx| idx.table != name);
        Ok(())
    }

    pub fn get_table(&self, name: &str) -> Result<&TableSchema> {
        self.tables
            .get(name)
            .ok_or_else(|| MiniDbError::TableNotFound(name.to_string()))
    }

    pub fn get_table_mut(&mut self, name: &str) -> Result<&mut TableSchema> {
        self.tables
            .get_mut(name)
            .ok_or_else(|| MiniDbError::TableNotFound(name.to_string()))
    }

    pub fn table_exists(&self, name: &str) -> bool {
        self.tables.contains_key(name)
    }

    pub fn table_names(&self) -> Vec<&str> {
        self.tables.keys().map(|s| s.as_str()).collect()
    }

    pub fn all_tables(&self) -> &HashMap<String, TableSchema> {
        &self.tables
    }

    pub fn all_tables_mut(&mut self) -> &mut HashMap<String, TableSchema> {
        &mut self.tables
    }

    pub fn create_index(&mut self, idx: IndexDef) -> Result<()> {
        if self.indexes.contains_key(&idx.name) {
            return Err(MiniDbError::IndexAlreadyExists(idx.name.clone()));
        }
        if !self.tables.contains_key(&idx.table) {
            return Err(MiniDbError::TableNotFound(idx.table.clone()));
        }
        self.indexes.insert(idx.name.clone(), idx);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<()> {
        if self.indexes.remove(name).is_none() {
            return Err(MiniDbError::IndexNotFound(name.to_string()));
        }
        Ok(())
    }

    pub fn get_index(&self, name: &str) -> Result<&IndexDef> {
        self.indexes
            .get(name)
            .ok_or_else(|| MiniDbError::IndexNotFound(name.to_string()))
    }

    pub fn indexes_for_table(&self, table: &str) -> Vec<&IndexDef> {
        self.indexes.values().filter(|i| i.table == table).collect()
    }

    pub fn find_index_on_column(&self, table: &str, column: &str) -> Option<&IndexDef> {
        self.indexes
            .values()
            .find(|i| i.table == table && i.column == column)
    }

    pub fn index_names(&self) -> Vec<&str> {
        self.indexes.keys().map(|s| s.as_str()).collect()
    }

    pub fn all_indexes(&self) -> &HashMap<String, IndexDef> {
        &self.indexes
    }

    pub fn all(&self) -> (&HashMap<String, TableSchema>, &HashMap<String, IndexDef>) {
        (&self.tables, &self.indexes)
    }

    pub fn merge(
        &mut self,
        tables: HashMap<String, TableSchema>,
        indexes: HashMap<String, IndexDef>,
    ) {
        for (name, schema) in tables {
            self.tables.insert(name, schema);
        }
        for (name, idx) in indexes {
            self.indexes.insert(name, idx);
        }
    }

    pub fn into_parts(self) -> (HashMap<String, TableSchema>, HashMap<String, IndexDef>) {
        (self.tables, self.indexes)
    }

    pub fn resolve_column(&self, table_name: &str, col_name: &str) -> Result<(usize, &ColumnDef)> {
        let schema = self.get_table(table_name)?;
        let idx = schema
            .column_index(col_name)
            .ok_or_else(|| MiniDbError::ColumnNotFound(format!("{}.{}", table_name, col_name)))?;
        Ok((idx, &schema.columns[idx]))
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
    use crate::types::ColumnType;

    fn make_schema(name: &str) -> TableSchema {
        TableSchema {
            name: name.to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    col_type: ColumnType::Int,
                    nullable: false,
                    primary_key: true,
                    unique: false,
                    default: None,
                },
                ColumnDef {
                    name: "name".into(),
                    col_type: ColumnType::Text,
                    nullable: true,
                    primary_key: false,
                    unique: false,
                    default: None,
                },
            ],
            row_count: 0,
        }
    }

    #[test]
    fn test_create_and_get_table() {
        let mut cat = Catalog::new();
        cat.create_table(make_schema("t1")).unwrap();
        assert!(cat.table_exists("t1"));
        assert_eq!(cat.get_table("t1").unwrap().columns.len(), 2);
    }

    #[test]
    fn test_duplicate_table() {
        let mut cat = Catalog::new();
        cat.create_table(make_schema("t1")).unwrap();
        assert!(cat.create_table(make_schema("t1")).is_err());
    }

    #[test]
    fn test_drop_table() {
        let mut cat = Catalog::new();
        cat.create_table(make_schema("t1")).unwrap();
        cat.drop_table("t1").unwrap();
        assert!(!cat.table_exists("t1"));
    }

    #[test]
    fn test_drop_nonexistent() {
        let mut cat = Catalog::new();
        assert!(cat.drop_table("nope").is_err());
    }

    #[test]
    fn test_create_index() {
        let mut cat = Catalog::new();
        cat.create_table(make_schema("t1")).unwrap();
        cat.create_index(IndexDef {
            name: "idx1".into(),
            table: "t1".into(),
            column: "id".into(),
        })
        .unwrap();
        assert!(cat.find_index_on_column("t1", "id").is_some());
    }

    #[test]
    fn test_drop_table_cascades_indexes() {
        let mut cat = Catalog::new();
        cat.create_table(make_schema("t1")).unwrap();
        cat.create_index(IndexDef {
            name: "idx1".into(),
            table: "t1".into(),
            column: "id".into(),
        })
        .unwrap();
        cat.drop_table("t1").unwrap();
        assert!(cat.index_names().is_empty());
    }

    #[test]
    fn test_resolve_column() {
        let mut cat = Catalog::new();
        cat.create_table(make_schema("t1")).unwrap();
        let (idx, col) = cat.resolve_column("t1", "name").unwrap();
        assert_eq!(idx, 1);
        assert_eq!(col.name, "name");
    }
}

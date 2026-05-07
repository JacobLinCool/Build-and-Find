use crate::error::{MiniDbError, Result};
use crate::storage::btree::BTreeIndex;
use crate::types::ColumnDef;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TableInfo {
    pub name: String,
    pub columns: Vec<ColumnDef>,
}

impl TableInfo {
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn has_column(&self, name: &str) -> bool {
        self.columns
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone)]
pub struct IndexInfo {
    pub name: String,
    pub table: String,
    pub column: String,
}

pub struct Catalog {
    tables: HashMap<String, TableInfo>,
    indexes: HashMap<String, IndexInfo>,
    /// index_name -> BTreeIndex (owned)
    pub btree_indexes: HashMap<String, BTreeIndex>,
}

impl Catalog {
    pub fn new() -> Self {
        Catalog {
            tables: HashMap::new(),
            indexes: HashMap::new(),
            btree_indexes: HashMap::new(),
        }
    }

    pub fn create_table(&mut self, info: TableInfo) -> Result<()> {
        if self.tables.contains_key(&info.name) {
            return Err(MiniDbError::Sql(format!(
                "table '{}' already exists",
                info.name
            )));
        }
        log::info!(
            "Created table '{}' with {} columns",
            info.name,
            info.columns.len()
        );
        self.tables.insert(info.name.clone(), info);
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        if self.tables.remove(name).is_none() {
            return Err(MiniDbError::Sql(format!("table '{}' does not exist", name)));
        }
        // Drop associated indexes
        let idx_names: Vec<String> = self
            .indexes
            .iter()
            .filter(|(_, info)| info.table == name)
            .map(|(n, _)| n.clone())
            .collect();
        for idx_name in &idx_names {
            self.indexes.remove(idx_name);
            self.btree_indexes.remove(idx_name);
        }
        log::info!("Dropped table '{}'", name);
        Ok(())
    }

    pub fn get_table(&self, name: &str) -> Result<&TableInfo> {
        self.tables
            .get(name)
            .ok_or_else(|| MiniDbError::Sql(format!("table '{}' does not exist", name)))
    }

    pub fn get_table_mut(&mut self, name: &str) -> Result<&mut TableInfo> {
        self.tables
            .get_mut(name)
            .ok_or_else(|| MiniDbError::Sql(format!("table '{}' does not exist", name)))
    }

    pub fn table_exists(&self, name: &str) -> bool {
        self.tables.contains_key(name)
    }

    pub fn all_table_names(&self) -> Vec<String> {
        self.tables.keys().cloned().collect()
    }

    pub fn all_tables(&self) -> Vec<&TableInfo> {
        self.tables.values().collect()
    }

    pub fn create_index(&mut self, name: String, table: String, column: String) -> Result<()> {
        if self.indexes.contains_key(&name) {
            return Err(MiniDbError::Sql(format!("index '{}' already exists", name)));
        }
        if !self.tables.contains_key(&table) {
            return Err(MiniDbError::Sql(format!(
                "table '{}' does not exist",
                table
            )));
        }
        let info = IndexInfo {
            name: name.clone(),
            table: table.clone(),
            column: column.clone(),
        };
        let btree = BTreeIndex::new(name.clone(), table, column);
        self.indexes.insert(name.clone(), info);
        self.btree_indexes.insert(name.clone(), btree);
        log::info!("Created index '{}'", name);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<()> {
        if self.indexes.remove(name).is_none() {
            return Err(MiniDbError::Sql(format!("index '{}' does not exist", name)));
        }
        self.btree_indexes.remove(name);
        log::info!("Dropped index '{}'", name);
        Ok(())
    }

    pub fn get_index(&self, name: &str) -> Option<&IndexInfo> {
        self.indexes.get(name)
    }

    pub fn all_index_names(&self) -> Vec<String> {
        self.indexes.keys().cloned().collect()
    }

    pub fn all_indexes(&self) -> Vec<&IndexInfo> {
        self.indexes.values().collect()
    }

    /// Find an index that matches the given table and column
    pub fn find_index(&self, table: &str, column: &str) -> Option<&BTreeIndex> {
        self.indexes
            .values()
            .find(|info| info.table == table && info.column == column)
            .and_then(|info| self.btree_indexes.get(&info.name))
    }

    pub fn find_index_mut(&mut self, table: &str, column: &str) -> Option<&mut BTreeIndex> {
        let name = self
            .indexes
            .values()
            .find(|info| info.table == table && info.column == column)
            .map(|info| info.name.clone())?;
        self.btree_indexes.get_mut(&name)
    }

    pub fn schema_string(&self, table_name: &str) -> Result<String> {
        let table = self.get_table(table_name)?;
        let mut s = format!("CREATE TABLE {} (\n", table.name);
        for (i, col) in table.columns.iter().enumerate() {
            s.push_str(&format!("  {} {}", col.name, col.col_type));
            if col.primary_key {
                s.push_str(" PRIMARY KEY");
            }
            if col.not_null && !col.primary_key {
                s.push_str(" NOT NULL");
            }
            if col.unique && !col.primary_key {
                s.push_str(" UNIQUE");
            }
            if let Some(ref def) = col.default {
                s.push_str(&format!(" DEFAULT {}", def));
            }
            if i < table.columns.len() - 1 {
                s.push(',');
            }
            s.push('\n');
        }
        s.push_str(")");
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ColumnType;

    #[test]
    fn test_create_drop_table() {
        let mut catalog = Catalog::new();
        let info = TableInfo {
            name: "users".into(),
            columns: vec![
                ColumnDef::new("id".into(), ColumnType::Int),
                ColumnDef::new("name".into(), ColumnType::Text),
            ],
        };
        catalog.create_table(info).unwrap();
        assert!(catalog.table_exists("users"));

        catalog.drop_table("users").unwrap();
        assert!(!catalog.table_exists("users"));
    }

    #[test]
    fn test_create_index() {
        let mut catalog = Catalog::new();
        let info = TableInfo {
            name: "users".into(),
            columns: vec![ColumnDef::new("id".into(), ColumnType::Int)],
        };
        catalog.create_table(info).unwrap();
        catalog
            .create_index("idx_id".into(), "users".into(), "id".into())
            .unwrap();
        assert!(catalog.find_index("users", "id").is_some());
    }
}

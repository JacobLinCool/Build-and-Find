//! In-memory catalog of tables and indexes.
//!
//! The catalog is reconstructed from the WAL at startup. It is kept in memory
//! during the lifetime of the process and persisted by writing every DDL and
//! DML operation to the WAL ahead of any data-file mutation.

use crate::error::{MiniDbError, Result};
use crate::storage::page::PageId;
use crate::types::{ColumnDef, DataType};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Table {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub heap_root: PageId,
    pub indexes: Vec<String>,
}

impl Table {
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn column(&self, name: &str) -> Option<&ColumnDef> {
        self.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn primary_key_index(&self) -> Option<usize> {
        self.columns.iter().position(|c| c.primary_key)
    }
}

#[derive(Debug, Clone)]
pub struct Index {
    pub name: String,
    pub table: String,
    pub column: String,
    pub unique: bool,
    pub root: PageId,
}

#[derive(Debug, Default)]
pub struct Catalog {
    tables: HashMap<String, Table>,
    indexes: HashMap<String, Index>,
}

impl Catalog {
    pub fn new() -> Self {
        Catalog::default()
    }

    pub fn create_table(&mut self, table: Table) -> Result<()> {
        let key = table.name.to_ascii_lowercase();
        if self.tables.contains_key(&key) {
            return Err(MiniDbError::catalog(format!(
                "table already exists: {}",
                table.name
            )));
        }
        // Validate column definitions.
        let mut seen = std::collections::HashSet::new();
        let mut pk_count = 0;
        for c in &table.columns {
            if !seen.insert(c.name.to_ascii_lowercase()) {
                return Err(MiniDbError::catalog(format!(
                    "duplicate column name: {}",
                    c.name
                )));
            }
            if c.primary_key {
                pk_count += 1;
            }
        }
        if pk_count > 1 {
            return Err(MiniDbError::catalog(
                "multiple PRIMARY KEY columns are not supported",
            ));
        }
        self.tables.insert(key, table);
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<Table> {
        let key = name.to_ascii_lowercase();
        let removed = self
            .tables
            .remove(&key)
            .ok_or_else(|| MiniDbError::catalog(format!("no such table: {}", name)))?;
        // Cascade-drop any indexes belonging to this table.
        let to_drop: Vec<String> = self
            .indexes
            .iter()
            .filter_map(|(k, idx)| {
                if idx.table.eq_ignore_ascii_case(name) {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect();
        for k in to_drop {
            self.indexes.remove(&k);
        }
        Ok(removed)
    }

    pub fn add_index(&mut self, index: Index) -> Result<()> {
        let key = index.name.to_ascii_lowercase();
        if self.indexes.contains_key(&key) {
            return Err(MiniDbError::catalog(format!(
                "index already exists: {}",
                index.name
            )));
        }
        let table_key = index.table.to_ascii_lowercase();
        let table = self
            .tables
            .get_mut(&table_key)
            .ok_or_else(|| MiniDbError::catalog(format!("no such table: {}", index.table)))?;
        if table.column(&index.column).is_none() {
            return Err(MiniDbError::catalog(format!(
                "no such column: {}.{}",
                index.table, index.column
            )));
        }
        if !table
            .indexes
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&index.name))
        {
            table.indexes.push(index.name.clone());
        }
        self.indexes.insert(key, index);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<Index> {
        let key = name.to_ascii_lowercase();
        let removed = self
            .indexes
            .remove(&key)
            .ok_or_else(|| MiniDbError::catalog(format!("no such index: {}", name)))?;
        let table_key = removed.table.to_ascii_lowercase();
        if let Some(t) = self.tables.get_mut(&table_key) {
            t.indexes.retain(|n| !n.eq_ignore_ascii_case(name));
        }
        Ok(removed)
    }

    pub fn get_table(&self, name: &str) -> Result<&Table> {
        self.tables
            .get(&name.to_ascii_lowercase())
            .ok_or_else(|| MiniDbError::catalog(format!("no such table: {}", name)))
    }

    pub fn get_table_mut(&mut self, name: &str) -> Result<&mut Table> {
        self.tables
            .get_mut(&name.to_ascii_lowercase())
            .ok_or_else(|| MiniDbError::catalog(format!("no such table: {}", name)))
    }

    pub fn get_index(&self, name: &str) -> Result<&Index> {
        self.indexes
            .get(&name.to_ascii_lowercase())
            .ok_or_else(|| MiniDbError::catalog(format!("no such index: {}", name)))
    }

    pub fn tables(&self) -> impl Iterator<Item = &Table> {
        self.tables.values()
    }

    pub fn indexes(&self) -> impl Iterator<Item = &Index> {
        self.indexes.values()
    }

    /// Find an index over `(table, column)` if one exists.
    pub fn find_index_for(&self, table: &str, column: &str) -> Option<&Index> {
        self.indexes
            .values()
            .find(|i| i.table.eq_ignore_ascii_case(table) && i.column.eq_ignore_ascii_case(column))
    }

    /// Index whose `unique=true` and is on the primary key column, if any.
    pub fn primary_index(&self, table: &str) -> Option<&Index> {
        let t = self.tables.get(&table.to_ascii_lowercase())?;
        let pk = t.columns.iter().find(|c| c.primary_key)?;
        self.find_index_for(table, &pk.name)
    }
}

/// Helper that lifts a default value plus the column type into a coerced value.
pub fn default_for_column(col: &ColumnDef) -> crate::types::Value {
    use crate::types::Value;
    match (&col.default, col.data_type) {
        (Some(v), _) => v.clone(),
        (None, DataType::Int)
        | (None, DataType::Real)
        | (None, DataType::Text)
        | (None, DataType::Bool) => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str, dt: DataType) -> ColumnDef {
        ColumnDef::new(name, dt)
    }

    #[test]
    fn create_drop_table() {
        let mut cat = Catalog::new();
        cat.create_table(Table {
            name: "t".into(),
            columns: vec![def("id", DataType::Int)],
            heap_root: PageId(1),
            indexes: vec![],
        })
        .unwrap();
        assert!(cat.get_table("t").is_ok());
        let dup = cat.create_table(Table {
            name: "T".into(),
            columns: vec![def("x", DataType::Int)],
            heap_root: PageId(2),
            indexes: vec![],
        });
        assert!(dup.is_err());
        cat.drop_table("T").unwrap();
        assert!(cat.get_table("t").is_err());
    }

    #[test]
    fn duplicate_column_rejected() {
        let mut cat = Catalog::new();
        let r = cat.create_table(Table {
            name: "t".into(),
            columns: vec![def("a", DataType::Int), def("A", DataType::Int)],
            heap_root: PageId(1),
            indexes: vec![],
        });
        assert!(r.is_err());
    }

    #[test]
    fn index_attachment() {
        let mut cat = Catalog::new();
        cat.create_table(Table {
            name: "t".into(),
            columns: vec![def("id", DataType::Int)],
            heap_root: PageId(1),
            indexes: vec![],
        })
        .unwrap();
        cat.add_index(Index {
            name: "idx_t_id".into(),
            table: "t".into(),
            column: "id".into(),
            unique: false,
            root: PageId(2),
        })
        .unwrap();
        assert!(cat.find_index_for("t", "id").is_some());
        cat.drop_index("idx_t_id").unwrap();
        assert!(cat.find_index_for("t", "id").is_none());
    }
}

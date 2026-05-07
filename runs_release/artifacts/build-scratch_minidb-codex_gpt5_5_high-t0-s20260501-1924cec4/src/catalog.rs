use crate::error::{MiniDbError, Result};
use crate::parser::ast::{ColumnDef, Expr};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub tables: BTreeMap<String, TableSchema>,
    pub indexes: BTreeMap<String, IndexSchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub checks: Vec<Expr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSchema {
    pub name: String,
    pub table: String,
    pub column: String,
}

impl Catalog {
    pub fn create_table(
        &mut self,
        name: String,
        columns: Vec<ColumnDef>,
        checks: Vec<Expr>,
    ) -> Result<()> {
        let key = normalize(&name);
        if self.tables.contains_key(&key) {
            return Err(MiniDbError::Sql(format!("table {name} already exists")));
        }
        let mut seen = BTreeMap::new();
        for col in &columns {
            let col_key = normalize(&col.name);
            if seen.insert(col_key, ()).is_some() {
                return Err(MiniDbError::Sql("duplicate column name".into()));
            }
        }
        self.tables.insert(
            key,
            TableSchema {
                name,
                columns,
                checks,
            },
        );
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<Vec<String>> {
        let key = normalize(name);
        if self.tables.remove(&key).is_none() {
            return Err(MiniDbError::Sql(format!("unknown table {name}")));
        }
        let removed = self
            .indexes
            .iter()
            .filter_map(|(idx_name, idx)| {
                if normalize(&idx.table) == key {
                    Some(idx_name.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for idx in &removed {
            self.indexes.remove(idx);
        }
        Ok(removed)
    }

    pub fn create_index(&mut self, name: String, table: String, column: String) -> Result<()> {
        let idx_key = normalize(&name);
        if self.indexes.contains_key(&idx_key) {
            return Err(MiniDbError::Sql(format!("index {name} already exists")));
        }
        let table_schema = self.table(&table)?;
        table_schema.column_index(&column)?;
        self.indexes.insert(
            idx_key,
            IndexSchema {
                name,
                table,
                column,
            },
        );
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<IndexSchema> {
        self.indexes
            .remove(&normalize(name))
            .ok_or_else(|| MiniDbError::Sql(format!("unknown index {name}")))
    }

    pub fn table(&self, name: &str) -> Result<&TableSchema> {
        self.tables
            .get(&normalize(name))
            .ok_or_else(|| MiniDbError::Sql(format!("unknown table {name}")))
    }

    pub fn index_on_column(&self, table: &str, column: &str) -> Option<&IndexSchema> {
        let table_key = normalize(table);
        let col_key = normalize(column);
        self.indexes
            .values()
            .find(|idx| normalize(&idx.table) == table_key && normalize(&idx.column) == col_key)
    }
}

impl TableSchema {
    pub fn column_index(&self, name: &str) -> Result<usize> {
        let key = normalize(name);
        self.columns
            .iter()
            .position(|c| normalize(&c.name) == key)
            .ok_or_else(|| MiniDbError::Sql(format!("unknown column {name}")))
    }

    pub fn column_names(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.name.clone()).collect()
    }
}

pub fn normalize(name: &str) -> String {
    name.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DataType;

    #[test]
    fn rejects_duplicate_tables() {
        let mut catalog = Catalog::default();
        let col = ColumnDef {
            name: "id".into(),
            data_type: DataType::Int,
            primary_key: true,
            not_null: true,
            unique: true,
            default: None,
        };
        catalog
            .create_table("t".into(), vec![col.clone()], Vec::new())
            .unwrap();
        assert!(catalog
            .create_table("T".into(), vec![col], Vec::new())
            .is_err());
    }
}

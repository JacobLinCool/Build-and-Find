use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{
    error::{MiniDbError, Result},
    parser::ast::Expr,
    types::{ColumnType, Value},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub ty: ColumnType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexMeta {
    pub name: String,
    pub table: String,
    pub column: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<Column>,
    pub checks: Vec<Expr>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub tables: BTreeMap<String, TableSchema>,
    pub indexes: BTreeMap<String, IndexMeta>,
}

impl Catalog {
    pub fn table(&self, name: &str) -> Result<&TableSchema> {
        self.tables
            .get(&name.to_ascii_lowercase())
            .ok_or_else(|| MiniDbError::Sql(format!("unknown table '{name}'")))
    }

    pub fn index_on(&self, table: &str, column: &str) -> Option<&IndexMeta> {
        self.indexes
            .values()
            .find(|i| i.table == table && i.column == column)
    }

    pub fn create_table(&mut self, mut schema: TableSchema) -> Result<()> {
        schema.name = schema.name.to_ascii_lowercase();
        for c in &mut schema.columns {
            c.name = c.name.to_ascii_lowercase();
        }
        if self.tables.contains_key(&schema.name) {
            return Err(MiniDbError::Sql(format!(
                "table '{}' already exists",
                schema.name
            )));
        }
        self.tables.insert(schema.name.clone(), schema);
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        let key = name.to_ascii_lowercase();
        self.tables
            .remove(&key)
            .ok_or_else(|| MiniDbError::Sql(format!("unknown table '{name}'")))?;
        self.indexes.retain(|_, idx| idx.table != key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_tracks_tables() {
        let mut c = Catalog::default();
        c.create_table(TableSchema {
            name: "T".into(),
            columns: vec![],
            checks: vec![],
        })
        .unwrap();
        assert!(c.table("t").is_ok());
    }
}

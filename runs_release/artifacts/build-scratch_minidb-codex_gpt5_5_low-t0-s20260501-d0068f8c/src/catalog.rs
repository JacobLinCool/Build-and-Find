use crate::{
    error::{MiniDbError, Result},
    types::Column,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<Column>,
    pub checks: Vec<crate::parser::ast::Expr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSchema {
    pub name: String,
    pub table: String,
    pub column: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub tables: BTreeMap<String, TableSchema>,
    pub indexes: BTreeMap<String, IndexSchema>,
}

impl Catalog {
    pub fn create_table(&mut self, table: TableSchema) -> Result<()> {
        if self.tables.contains_key(&table.name) {
            return Err(MiniDbError::Sql(format!(
                "table {} already exists",
                table.name
            )));
        }
        self.tables.insert(table.name.clone(), table);
        Ok(())
    }
    pub fn table(&self, name: &str) -> Result<&TableSchema> {
        self.tables
            .get(name)
            .ok_or_else(|| MiniDbError::Sql(format!("unknown table {name}")))
    }
    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        self.tables
            .remove(name)
            .ok_or_else(|| MiniDbError::Sql(format!("unknown table {name}")))?;
        self.indexes.retain(|_, idx| idx.table != name);
        Ok(())
    }
    pub fn index_on(&self, table: &str, column: &str) -> Option<&IndexSchema> {
        self.indexes
            .values()
            .find(|i| i.table == table && i.column == column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_table_errors() {
        assert!(Catalog::default().table("x").is_err());
    }
}

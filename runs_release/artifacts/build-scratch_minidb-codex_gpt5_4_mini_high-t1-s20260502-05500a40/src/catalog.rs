use crate::parser::ast::Expr;
use crate::types::{Column, Schema, SchemaColumn};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<Column>,
    pub checks: Vec<Expr>,
}

impl TableSchema {
    pub fn schema(&self) -> Schema {
        Schema {
            columns: self
                .columns
                .iter()
                .map(|column| SchemaColumn {
                    table: Some(self.name.clone()),
                    name: column.name.clone(),
                    col_type: column.col_type.clone(),
                })
                .collect(),
        }
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|col| col.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexSchema {
    pub name: String,
    pub table: String,
    pub column: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Catalog {
    pub tables: BTreeMap<String, TableSchema>,
    pub indexes: BTreeMap<String, IndexSchema>,
}

impl Catalog {
    pub fn create_table(&mut self, schema: TableSchema) {
        self.tables.insert(schema.name.clone(), schema);
    }

    pub fn drop_table(&mut self, name: &str) {
        self.tables.remove(name);
        self.indexes.retain(|_, index| index.table != name);
    }

    pub fn create_index(&mut self, index: IndexSchema) {
        self.indexes.insert(index.name.clone(), index);
    }

    pub fn drop_index(&mut self, name: &str) {
        self.indexes.remove(name);
    }

    pub fn table(&self, name: &str) -> Option<&TableSchema> {
        self.tables.get(name)
    }

    pub fn table_mut(&mut self, name: &str) -> Option<&mut TableSchema> {
        self.tables.get_mut(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ColumnType, Value};

    #[test]
    fn table_schema_builds_schema() {
        let schema = TableSchema {
            name: "t".into(),
            columns: vec![Column {
                name: "id".into(),
                col_type: ColumnType::Int,
                not_null: true,
                unique: true,
                primary_key: true,
                default: Some(Value::Int(1)),
            }],
            checks: vec![],
        };
        assert_eq!(schema.schema().columns.len(), 1);
    }
}

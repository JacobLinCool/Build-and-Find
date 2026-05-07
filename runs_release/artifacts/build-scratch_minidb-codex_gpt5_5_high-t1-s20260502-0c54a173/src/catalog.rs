use crate::parser::ast::Expr;
use crate::types::{Column, ColumnType, Value};
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
    pub columns: Vec<Column>,
    pub checks: Vec<Expr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSchema {
    pub name: String,
    pub table: String,
    pub column: String,
}

impl TableSchema {
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn qualified_columns(&self) -> Vec<String> {
        self.columns
            .iter()
            .map(|c| format!("{}.{}", self.name, c.name))
            .collect()
    }
}

impl From<crate::parser::ast::ColumnDef> for Column {
    fn from(def: crate::parser::ast::ColumnDef) -> Self {
        Column {
            name: def.name,
            ty: def.ty,
            primary_key: def.primary_key,
            not_null: def.not_null,
            unique: def.unique,
            default: def.default,
        }
    }
}

pub fn type_sql(ty: ColumnType) -> &'static str {
    match ty {
        ColumnType::Int => "INT",
        ColumnType::Real => "REAL",
        ColumnType::Text => "TEXT",
        ColumnType::Bool => "BOOL",
    }
}

pub fn value_sql(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Int(v) => v.to_string(),
        Value::Real(v) => v.to_string(),
        Value::Text(v) => format!("'{}'", v.replace('\'', "''")),
        Value::Bool(v) => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_column_case_insensitively() {
        let schema = TableSchema {
            name: "t".into(),
            columns: vec![Column {
                name: "Id".into(),
                ty: ColumnType::Int,
                primary_key: false,
                not_null: false,
                unique: false,
                default: Value::Null,
            }],
            checks: vec![],
        };
        assert_eq!(schema.column_index("id"), Some(0));
    }
}

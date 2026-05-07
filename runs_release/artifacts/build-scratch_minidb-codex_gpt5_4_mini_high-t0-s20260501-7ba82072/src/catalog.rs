use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::types::{Column, Literal, TableConstraint};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<Column>,
    pub table_constraints: Vec<TableConstraint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexSchema {
    pub name: String,
    pub table: String,
    pub column: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
        self.indexes.retain(|_, idx| idx.table != name);
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

    pub fn index_on_column(&self, table: &str, column: &str) -> Option<&IndexSchema> {
        self.indexes
            .values()
            .find(|idx| idx.table == table && idx.column == column)
    }

    pub fn list_tables(&self) -> Vec<String> {
        self.tables.keys().cloned().collect()
    }

    pub fn list_indexes(&self) -> Vec<String> {
        self.indexes.keys().cloned().collect()
    }

    pub fn schema_text(&self, table: Option<&str>) -> String {
        let mut out = String::new();
        match table {
            Some(name) => {
                if let Some(schema) = self.tables.get(name) {
                    out.push_str(&table_ddl(schema));
                    out.push('\n');
                }
            }
            None => {
                for schema in self.tables.values() {
                    out.push_str(&table_ddl(schema));
                    out.push('\n');
                }
            }
        }
        out
    }
}

fn table_ddl(schema: &TableSchema) -> String {
    let mut parts = Vec::new();
    for col in &schema.columns {
        let mut item = format!("{} {}", col.name, col.col_type.name());
        if col.constraints.primary_key {
            item.push_str(" PRIMARY KEY");
        }
        if col.constraints.not_null {
            item.push_str(" NOT NULL");
        }
        if col.constraints.unique {
            item.push_str(" UNIQUE");
        }
        if let Some(default) = &col.constraints.default {
            item.push_str(&format!(" DEFAULT {}", literal_text(default)));
        }
        parts.push(item);
    }
    for constraint in &schema.table_constraints {
        match constraint {
            TableConstraint::Check(expr) => parts.push(format!("CHECK ({expr:?})")),
        }
    }
    format!("CREATE TABLE {} ({});", schema.name, parts.join(", "))
}

fn literal_text(literal: &Literal) -> String {
    match literal {
        Literal::Null => "NULL".to_string(),
        Literal::Int(v) => v.to_string(),
        Literal::Real(v) => v.to_string(),
        Literal::Text(v) => format!("'{}'", v.replace('\'', "''")),
        Literal::Bool(v) => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ColumnConstraint, ColumnType};

    #[test]
    fn catalog_tracks_tables() {
        let mut catalog = Catalog::default();
        catalog.create_table(TableSchema {
            name: "t".into(),
            columns: vec![Column {
                name: "id".into(),
                col_type: ColumnType::Int,
                constraints: ColumnConstraint::new(),
            }],
            table_constraints: vec![],
        });
        assert_eq!(catalog.list_tables(), vec!["t".to_string()]);
    }
}

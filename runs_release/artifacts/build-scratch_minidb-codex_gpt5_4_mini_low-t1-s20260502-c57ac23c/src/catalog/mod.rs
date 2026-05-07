use serde::{Deserialize, Serialize};

use crate::parser::ast::Expr;
use crate::types::Column;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Catalog {
    pub tables: std::collections::BTreeMap<String, TableMetadata>,
    pub indexes: std::collections::BTreeMap<String, IndexMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableMetadata {
    pub name: String,
    pub columns: Vec<Column>,
    pub checks: Vec<Expr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMetadata {
    pub name: String,
    pub table: String,
    pub column: String,
}

impl TableMetadata {
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ColumnType;
    #[test]
    fn table_lookup_works() {
        let t = TableMetadata {
            name: "t".into(),
            columns: vec![Column {
                table: None,
                name: "id".into(),
                col_type: ColumnType::Int,
                primary_key: false,
                not_null: false,
                unique: false,
                default: None,
            }],
            checks: vec![],
        };
        assert_eq!(t.column_index("id"), Some(0));
    }
}

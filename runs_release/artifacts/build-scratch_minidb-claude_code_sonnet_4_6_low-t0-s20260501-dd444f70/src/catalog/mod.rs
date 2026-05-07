use crate::error::{MiniDbError, Result};
use crate::parser::ast::{ColumnConstraint, CreateIndexStmt, CreateTableStmt, TableConstraint};
use crate::types::{ColumnDef, DataType, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub primary_key: Option<Vec<String>>,
    pub check_exprs: Vec<String>,
    pub root_page: u64,
}

impl TableSchema {
    pub fn col_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn col_by_name(&self, name: &str) -> Option<&ColumnDef> {
        self.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSchema {
    pub name: String,
    pub table: String,
    pub column: String,
    pub unique: bool,
    pub root_page: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogData {
    pub tables: HashMap<String, TableSchema>,
    pub indexes: HashMap<String, IndexSchema>,
    pub next_page: u64,
}

impl CatalogData {
    pub fn new() -> Self {
        CatalogData {
            tables: HashMap::new(),
            indexes: HashMap::new(),
            next_page: 1,
        }
    }
}

pub struct Catalog {
    pub data: CatalogData,
}

impl Catalog {
    pub fn new() -> Self {
        Catalog {
            data: CatalogData::new(),
        }
    }

    pub fn alloc_page(&mut self) -> u64 {
        let p = self.data.next_page;
        self.data.next_page += 1;
        p
    }

    pub fn create_table(&mut self, stmt: &CreateTableStmt) -> Result<()> {
        let name_lower = stmt.table.to_lowercase();
        if self.data.tables.contains_key(&name_lower) {
            if stmt.if_not_exists {
                return Ok(());
            }
            return Err(MiniDbError::DuplicateTable(stmt.table.clone()));
        }

        let mut columns = Vec::new();
        let mut pk_cols = Vec::new();

        for col_spec in &stmt.columns {
            let mut col = ColumnDef::new(&col_spec.name, col_spec.data_type.clone());
            for constraint in &col_spec.constraints {
                match constraint {
                    ColumnConstraint::PrimaryKey => {
                        col.primary_key = true;
                        col.nullable = false;
                        col.unique = true;
                        pk_cols.push(col_spec.name.clone());
                    }
                    ColumnConstraint::NotNull => {
                        col.nullable = false;
                    }
                    ColumnConstraint::Unique => {
                        col.unique = true;
                    }
                    ColumnConstraint::Default(expr) => {
                        col.default = eval_literal_expr(expr);
                    }
                    ColumnConstraint::Check(_) => {}
                }
            }
            columns.push(col);
        }

        for tc in &stmt.table_constraints {
            match tc {
                TableConstraint::PrimaryKey(cols) => {
                    pk_cols.extend(cols.clone());
                    for col_name in cols {
                        if let Some(col) = columns
                            .iter_mut()
                            .find(|c| c.name.eq_ignore_ascii_case(col_name))
                        {
                            col.primary_key = true;
                            col.nullable = false;
                            col.unique = true;
                        }
                    }
                }
                TableConstraint::Unique(cols) => {
                    for col_name in cols {
                        if let Some(col) = columns
                            .iter_mut()
                            .find(|c| c.name.eq_ignore_ascii_case(col_name))
                        {
                            col.unique = true;
                        }
                    }
                }
                TableConstraint::Check(_) => {}
            }
        }

        let root_page = self.alloc_page();

        let schema = TableSchema {
            name: stmt.table.clone(),
            columns,
            primary_key: if pk_cols.is_empty() {
                None
            } else {
                Some(pk_cols)
            },
            check_exprs: Vec::new(),
            root_page,
        };

        self.data.tables.insert(name_lower, schema);
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str, if_exists: bool) -> Result<()> {
        let name_lower = name.to_lowercase();
        if self.data.tables.remove(&name_lower).is_none() && !if_exists {
            return Err(MiniDbError::TableNotFound(name.to_string()));
        }
        // Also remove indexes on this table
        self.data
            .indexes
            .retain(|_, idx| !idx.table.eq_ignore_ascii_case(name));
        Ok(())
    }

    pub fn get_table(&self, name: &str) -> Result<&TableSchema> {
        self.data
            .tables
            .get(&name.to_lowercase())
            .ok_or_else(|| MiniDbError::TableNotFound(name.to_string()))
    }

    pub fn get_table_mut(&mut self, name: &str) -> Result<&mut TableSchema> {
        self.data
            .tables
            .get_mut(&name.to_lowercase())
            .ok_or_else(|| MiniDbError::TableNotFound(name.to_string()))
    }

    pub fn create_index(&mut self, stmt: &CreateIndexStmt) -> Result<()> {
        let name_lower = stmt.index_name.to_lowercase();
        if self.data.indexes.contains_key(&name_lower) {
            if stmt.if_not_exists {
                return Ok(());
            }
            return Err(MiniDbError::DuplicateIndex(stmt.index_name.clone()));
        }
        // Verify table and column exist
        let table = self.get_table(&stmt.table)?;
        if table.col_index(&stmt.column).is_none() {
            return Err(MiniDbError::ColumnNotFound(stmt.column.clone()));
        }

        let root_page = self.alloc_page();
        let idx = IndexSchema {
            name: stmt.index_name.clone(),
            table: stmt.table.clone(),
            column: stmt.column.clone(),
            unique: stmt.unique,
            root_page,
        };
        self.data.indexes.insert(name_lower, idx);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str, if_exists: bool) -> Result<()> {
        let name_lower = name.to_lowercase();
        if self.data.indexes.remove(&name_lower).is_none() && !if_exists {
            return Err(MiniDbError::IndexNotFound(name.to_string()));
        }
        Ok(())
    }

    pub fn get_index(&self, name: &str) -> Result<&IndexSchema> {
        self.data
            .indexes
            .get(&name.to_lowercase())
            .ok_or_else(|| MiniDbError::IndexNotFound(name.to_string()))
    }

    pub fn indexes_for_table(&self, table: &str) -> Vec<&IndexSchema> {
        self.data
            .indexes
            .values()
            .filter(|idx| idx.table.eq_ignore_ascii_case(table))
            .collect()
    }

    pub fn serialize(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&self.data).map_err(MiniDbError::from)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self> {
        let catalog_data: CatalogData = serde_json::from_slice(data)
            .map_err(|e| MiniDbError::Storage(format!("catalog deserialize: {}", e)))?;
        Ok(Catalog { data: catalog_data })
    }
}

fn eval_literal_expr(expr: &crate::parser::ast::Expr) -> Option<Value> {
    match expr {
        crate::parser::ast::Expr::Lit(v) => Some(v.clone()),
        crate::parser::ast::Expr::UnaryOp(crate::parser::ast::UnaryOp::Neg, inner) => {
            if let crate::parser::ast::Expr::Lit(Value::Int(n)) = inner.as_ref() {
                Some(Value::Int(-n))
            } else if let crate::parser::ast::Expr::Lit(Value::Real(r)) = inner.as_ref() {
                Some(Value::Real(-r))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::Stmt;
    use crate::parser::parse;

    fn make_catalog() -> Catalog {
        Catalog::new()
    }

    #[test]
    fn test_create_and_get_table() {
        let mut cat = make_catalog();
        let stmts = parse("CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);").unwrap();
        if let Stmt::CreateTable(ct) = &stmts[0] {
            cat.create_table(ct).unwrap();
        }
        let schema = cat.get_table("users").unwrap();
        assert_eq!(schema.columns.len(), 2);
        assert!(!schema.columns[0].nullable);
    }

    #[test]
    fn test_duplicate_table_error() {
        let mut cat = make_catalog();
        let stmts = parse("CREATE TABLE t (id INT);").unwrap();
        if let Stmt::CreateTable(ct) = &stmts[0] {
            cat.create_table(ct).unwrap();
            assert!(cat.create_table(ct).is_err());
        }
    }

    #[test]
    fn test_drop_table() {
        let mut cat = make_catalog();
        let stmts = parse("CREATE TABLE t (id INT);").unwrap();
        if let Stmt::CreateTable(ct) = &stmts[0] {
            cat.create_table(ct).unwrap();
        }
        cat.drop_table("t", false).unwrap();
        assert!(cat.get_table("t").is_err());
    }

    #[test]
    fn test_catalog_serialize_roundtrip() {
        let mut cat = make_catalog();
        let stmts = parse("CREATE TABLE t (id INT, name TEXT);").unwrap();
        if let Stmt::CreateTable(ct) = &stmts[0] {
            cat.create_table(ct).unwrap();
        }
        let bytes = cat.serialize().unwrap();
        let cat2 = Catalog::deserialize(&bytes).unwrap();
        assert!(cat2.get_table("t").is_ok());
    }
}

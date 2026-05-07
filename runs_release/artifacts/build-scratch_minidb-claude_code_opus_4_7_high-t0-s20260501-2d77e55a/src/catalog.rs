//! Schema catalog: tables, columns, constraints, and secondary indexes.
//!
//! The catalog is serialized to a JSON sidecar so it survives restarts and
//! is human-readable. Logical operators consult the catalog for column
//! schema, constraints, and index hints; the executor never inspects raw
//! storage to discover columns.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{MiniDbError, Result};
use crate::parser::ast::Expr;
use crate::storage::PageId;
use crate::types::{DataType, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColumnSchema {
    pub name: String,
    pub dtype: DataType,
    pub not_null: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnSchema>,
    /// Table-level CHECK predicates serialized to source text for replay.
    pub checks: Vec<String>,
    /// Head page id of the heap chain.
    pub heap_head: PageId,
}

impl TableSchema {
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn column(&self, name: &str) -> Option<&ColumnSchema> {
        self.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Get the parsed CHECK predicates. Returns parse errors if any check is
    /// malformed (which should be impossible because they were validated at
    /// CREATE TABLE time).
    pub fn parsed_checks(&self) -> Result<Vec<Expr>> {
        let mut out = Vec::new();
        for s in &self.checks {
            let mut p = crate::parser::Parser::new(crate::lexer::Lexer::new(s).tokenize()?);
            let e = p.parse_expr()?;
            out.push(e);
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexSchema {
    pub name: String,
    pub table: String,
    pub column: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub tables: BTreeMap<String, TableSchema>,
    pub indexes: BTreeMap<String, IndexSchema>,
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_table(&mut self, t: TableSchema) -> Result<()> {
        let key = t.name.to_ascii_lowercase();
        if self.tables.contains_key(&key) {
            return Err(MiniDbError::catalog(format!(
                "table '{}' already exists",
                t.name
            )));
        }
        self.tables.insert(key, t);
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<TableSchema> {
        let key = name.to_ascii_lowercase();
        let t = self
            .tables
            .remove(&key)
            .ok_or_else(|| MiniDbError::catalog(format!("table '{}' does not exist", name)))?;
        // Remove dependent indexes
        let dep: Vec<String> = self
            .indexes
            .iter()
            .filter(|(_, idx)| idx.table.eq_ignore_ascii_case(name))
            .map(|(k, _)| k.clone())
            .collect();
        for k in dep {
            self.indexes.remove(&k);
        }
        Ok(t)
    }

    pub fn get_table(&self, name: &str) -> Result<&TableSchema> {
        self.tables
            .get(&name.to_ascii_lowercase())
            .ok_or_else(|| MiniDbError::catalog(format!("table '{}' does not exist", name)))
    }

    pub fn add_index(&mut self, idx: IndexSchema) -> Result<()> {
        let key = idx.name.to_ascii_lowercase();
        if self.indexes.contains_key(&key) {
            return Err(MiniDbError::catalog(format!(
                "index '{}' already exists",
                idx.name
            )));
        }
        self.get_table(&idx.table)?;
        let table = self.get_table(&idx.table)?;
        if table.column(&idx.column).is_none() {
            return Err(MiniDbError::catalog(format!(
                "column '{}' not found on table '{}'",
                idx.column, idx.table
            )));
        }
        self.indexes.insert(key, idx);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<IndexSchema> {
        self.indexes
            .remove(&name.to_ascii_lowercase())
            .ok_or_else(|| MiniDbError::catalog(format!("index '{}' does not exist", name)))
    }

    pub fn indexes_for_column(&self, table: &str, column: &str) -> Vec<&IndexSchema> {
        self.indexes
            .values()
            .filter(|i| {
                i.table.eq_ignore_ascii_case(table) && i.column.eq_ignore_ascii_case(column)
            })
            .collect()
    }

    pub fn to_json(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec_pretty(self)?)
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> TableSchema {
        TableSchema {
            name: "t".into(),
            columns: vec![ColumnSchema {
                name: "id".into(),
                dtype: DataType::Int,
                not_null: true,
                primary_key: true,
                unique: false,
                default: None,
            }],
            checks: vec![],
            heap_head: PageId(1),
        }
    }

    #[test]
    fn add_drop_table() {
        let mut c = Catalog::new();
        c.add_table(schema()).unwrap();
        assert!(c.get_table("t").is_ok());
        c.drop_table("t").unwrap();
        assert!(c.get_table("t").is_err());
    }

    #[test]
    fn duplicate_table_rejected() {
        let mut c = Catalog::new();
        c.add_table(schema()).unwrap();
        assert!(c.add_table(schema()).is_err());
    }

    #[test]
    fn json_roundtrip() {
        let mut c = Catalog::new();
        c.add_table(schema()).unwrap();
        let bytes = c.to_json().unwrap();
        let c2 = Catalog::from_json(&bytes).unwrap();
        assert!(c2.get_table("t").is_ok());
    }
}

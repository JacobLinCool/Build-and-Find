pub mod delete;
pub mod engine;
pub mod filter;
pub mod index_scan;
pub mod insert;
pub mod limit;
pub mod nested_loop_join;
pub mod project;
pub mod seq_scan;
pub mod sort;
pub mod update;

use crate::error::Result;
use crate::types::{Row, Value};
use std::collections::HashMap;

/// A schema entry describing a single output column of an operator.
#[derive(Debug, Clone)]
pub struct SchemaEntry {
    pub name: String,
    pub table: Option<String>,
}

pub type Schema = Vec<SchemaEntry>;

/// The core Volcano-style iterator trait.
pub trait PhysicalOperator {
    fn schema(&self) -> &Schema;
    fn open(&mut self) -> Result<()>;
    fn next(&mut self) -> Result<Option<Row>>;
    fn close(&mut self) -> Result<()>;
}

/// Execution context: binds column names to values for a current row.
#[derive(Debug, Clone, Default)]
pub struct EvalContext {
    /// Column name -> value (for single-table contexts)
    pub columns: HashMap<String, Value>,
    /// (table, column) -> value (for join contexts)
    pub qualified: HashMap<(String, String), Value>,
}

impl EvalContext {
    pub fn from_row(schema: &Schema, row: &Row) -> Self {
        let mut ctx = EvalContext::default();
        for (i, entry) in schema.iter().enumerate() {
            let val = row.get(i).cloned().unwrap_or(Value::Null);
            ctx.columns.insert(entry.name.clone(), val.clone());
            if let Some(tbl) = &entry.table {
                ctx.qualified.insert((tbl.clone(), entry.name.clone()), val);
            }
        }
        ctx
    }

    pub fn lookup(&self, col: &str, table: Option<&str>) -> Value {
        if let Some(tbl) = table {
            if let Some(v) = self.qualified.get(&(tbl.to_string(), col.to_string())) {
                return v.clone();
            }
            // Try case-insensitive
            for ((t, c), v) in &self.qualified {
                if t.eq_ignore_ascii_case(tbl) && c.eq_ignore_ascii_case(col) {
                    return v.clone();
                }
            }
        }
        // Try unqualified
        if let Some(v) = self.columns.get(col) {
            return v.clone();
        }
        for (c, v) in &self.columns {
            if c.eq_ignore_ascii_case(col) {
                return v.clone();
            }
        }
        Value::Null
    }

    pub fn merge(&mut self, other: &EvalContext) {
        for (k, v) in &other.columns {
            self.columns.insert(k.clone(), v.clone());
        }
        for (k, v) in &other.qualified {
            self.qualified.insert(k.clone(), v.clone());
        }
    }
}

pub use crate::exec::delete::Delete;
pub use crate::exec::filter::Filter;
pub use crate::exec::index_scan::IndexScan;
pub use crate::exec::insert::Insert;
pub use crate::exec::limit::Limit;
pub use crate::exec::nested_loop_join::NestedLoopJoin;
pub use crate::exec::project::Project;
pub use crate::exec::seq_scan::SeqScan;
pub use crate::exec::sort::Sort;
pub use crate::exec::update::Update;

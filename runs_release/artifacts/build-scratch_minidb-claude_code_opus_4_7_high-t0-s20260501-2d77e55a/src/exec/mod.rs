//! Volcano-style physical execution engine.
//!
//! Operators implement [`PhysicalOperator`] with `open`, `next`, `close`,
//! and `schema()`. They compose as a tree and stream rows one at a time
//! (or, for DML, emit a single row containing the affected count).

pub mod expr;
pub mod operators;

use std::collections::{HashMap, HashSet};

use crate::catalog::Catalog;
use crate::error::Result;
use crate::storage::btree::PersistentBTree;
use crate::storage::pager::Pager;
use crate::types::{DataType, Value};

pub type Row = Vec<Value>;

#[derive(Clone, Debug)]
pub struct ColumnRef {
    pub table: Option<String>,
    pub name: String,
    pub dtype: Option<DataType>,
}

#[derive(Clone, Debug, Default)]
pub struct OperatorSchema {
    pub columns: Vec<ColumnRef>,
}

impl OperatorSchema {
    pub fn new(columns: Vec<ColumnRef>) -> Self {
        Self { columns }
    }
    pub fn len(&self) -> usize {
        self.columns.len()
    }
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }
    pub fn column_names(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.name.clone()).collect()
    }
}

pub struct ExecCtx<'a> {
    pub pager: &'a mut Pager,
    pub catalog: &'a mut Catalog,
    pub indexes: &'a mut HashMap<String, PersistentBTree>,
    pub catalog_dirty: &'a mut bool,
    pub indexes_dirty: &'a mut HashSet<String>,
}

pub trait PhysicalOperator {
    fn open(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()>;
    fn next(&mut self, ctx: &mut ExecCtx<'_>) -> Result<Option<Row>>;
    fn close(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()>;
    fn schema(&self) -> &OperatorSchema;
}

pub use operators::*;

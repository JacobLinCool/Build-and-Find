pub mod delete_op;
pub mod filter;
pub mod index_scan;
pub mod insert;
pub mod join;
pub mod limit;
pub mod project;
pub mod seq_scan;
pub mod sort;
pub mod update;

use crate::error::Result;
use crate::types::{Row, Value};

pub trait PhysicalOperator {
    fn open(&mut self) -> Result<()>;
    fn next(&mut self) -> Result<Option<Row>>;
    fn close(&mut self) -> Result<()>;
    fn schema(&self) -> Vec<String>;
}

pub struct ExecutionContext<'a> {
    pub db_path: &'a str,
    pub tables: Vec<String>,
    pub quiet: bool,
}

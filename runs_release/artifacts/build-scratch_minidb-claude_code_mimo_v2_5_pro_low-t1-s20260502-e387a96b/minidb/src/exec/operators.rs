use crate::error::Result;
use crate::types::Row;

/// Volcano-style physical operator trait.
/// Each operator produces one row at a time via next().
pub trait PhysicalOperator {
    fn open(&mut self) -> Result<()>;
    fn next(&mut self) -> Result<Option<Row>>;
    fn close(&mut self) -> Result<()>;
    fn output_columns(&self) -> Vec<String>;
}

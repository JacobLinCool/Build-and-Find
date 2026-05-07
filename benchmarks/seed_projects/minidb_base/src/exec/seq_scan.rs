//! Sequential scan over a table's heap file.

use super::{ExecContext, PhysicalOperator};
use crate::error::Result;
use crate::types::{Row, Schema};

pub struct SeqScan {
    table: String,
    alias: Option<String>,
    schema: Schema,
    buffer: Vec<Row>,
    cursor: usize,
}

impl SeqScan {
    pub fn new(table: String, alias: Option<String>, schema: Schema) -> Self {
        Self {
            table,
            alias,
            schema,
            buffer: Vec::new(),
            cursor: 0,
        }
    }

    pub fn table_name(&self) -> &str {
        &self.table
    }

    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }
}

impl PhysicalOperator for SeqScan {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        let tbl = ctx.catalog.table(&self.table)?;
        self.buffer = tbl.heap.iter().map(|(_, r)| r.clone()).collect();
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecContext<'_>) -> Result<Option<Row>> {
        if self.cursor >= self.buffer.len() {
            return Ok(None);
        }
        let row = self.buffer[self.cursor].clone();
        self.cursor += 1;
        Ok(Some(row))
    }

    fn close(&mut self, _ctx: &mut ExecContext<'_>) -> Result<()> {
        self.buffer.clear();
        Ok(())
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}

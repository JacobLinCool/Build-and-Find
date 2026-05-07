use crate::error::Result;
use crate::exec::{EvalContext, PhysicalOperator, Schema, SchemaEntry};
use crate::storage::StorageEngine;
use crate::types::{Row, RowId, Value};
use std::cell::RefCell;
use std::rc::Rc;

pub struct SeqScan {
    pub table: String,
    pub alias: Option<String>,
    schema: Schema,
    rows: Vec<(RowId, Row)>,
    cursor: usize,
    engine: *mut StorageEngine,
}

impl SeqScan {
    pub fn new(table: String, alias: Option<String>, engine: *mut StorageEngine) -> Result<Self> {
        let eng = unsafe { &mut *engine };
        let schema_def = if table == "__dual__" {
            vec![]
        } else {
            let ts = eng.catalog.get_table(&table)?;
            ts.columns
                .iter()
                .map(|c| SchemaEntry {
                    name: c.name.clone(),
                    table: Some(alias.clone().unwrap_or_else(|| table.clone())),
                })
                .collect()
        };
        Ok(SeqScan {
            table,
            alias,
            schema: schema_def,
            rows: Vec::new(),
            cursor: 0,
            engine,
        })
    }
}

impl PhysicalOperator for SeqScan {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self) -> Result<()> {
        let eng = unsafe { &mut *self.engine };
        if self.table == "__dual__" {
            // Emit exactly one empty row
            self.rows = vec![(0, vec![])];
        } else {
            self.rows = eng.scan_table(&self.table)?;
        }
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.cursor >= self.rows.len() {
            return Ok(None);
        }
        let (_, row) = self.rows[self.cursor].clone();
        self.cursor += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        self.rows.clear();
        Ok(())
    }
}

// We need SeqScan to expose row_ids for UPDATE/DELETE
impl SeqScan {
    pub fn next_with_id(&mut self) -> Result<Option<(RowId, Row)>> {
        if self.cursor >= self.rows.len() {
            return Ok(None);
        }
        let item = self.rows[self.cursor].clone();
        self.cursor += 1;
        Ok(Some(item))
    }
}

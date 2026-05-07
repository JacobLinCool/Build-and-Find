use crate::error::Result;
use crate::exec::{PhysicalOperator, Schema, SchemaEntry};
use crate::storage::StorageEngine;
use crate::types::{Row, Value};

pub struct Delete {
    pub table: String,
    pub input: Box<dyn PhysicalOperator>,
    schema: Schema,
    result: Option<Row>,
    done: bool,
    engine: *mut StorageEngine,
}

impl Delete {
    pub fn new(
        table: String,
        input: Box<dyn PhysicalOperator>,
        engine: *mut StorageEngine,
    ) -> Self {
        let schema = vec![SchemaEntry {
            name: "rows_affected".to_string(),
            table: None,
        }];
        Delete {
            table,
            input,
            schema,
            result: None,
            done: false,
            engine,
        }
    }
}

impl PhysicalOperator for Delete {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self) -> Result<()> {
        let eng = unsafe { &mut *self.engine };

        // Collect matching rows
        self.input.open()?;
        let input_schema = self.input.schema().to_vec();
        let mut input_rows = Vec::new();
        while let Some(row) = self.input.next()? {
            input_rows.push(row);
        }
        self.input.close()?;

        // Scan the table to get row_ids for matching rows
        let all_rows = eng.scan_table(&self.table)?;
        let mut deleted = 0i64;

        for (rid, row) in all_rows {
            if input_rows.iter().any(|ir| ir == &row) {
                eng.delete_row(&self.table, rid)?;
                deleted += 1;
            }
        }

        self.result = Some(vec![Value::Int(deleted)]);
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.done {
            return Ok(None);
        }
        self.done = true;
        Ok(self.result.clone())
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

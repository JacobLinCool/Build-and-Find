use crate::error::Result;
use crate::exec::filter::eval_expr;
use crate::exec::{EvalContext, PhysicalOperator, Schema, SchemaEntry};
use crate::parser::ast::Expr;
use crate::storage::StorageEngine;
use crate::types::{Row, RowId, Value};

pub struct Update {
    pub table: String,
    pub input: Box<dyn PhysicalOperator>,
    pub assignments: Vec<(String, Expr)>,
    schema: Schema,
    result: Option<Row>,
    done: bool,
    engine: *mut StorageEngine,
}

impl Update {
    pub fn new(
        table: String,
        input: Box<dyn PhysicalOperator>,
        assignments: Vec<(String, Expr)>,
        engine: *mut StorageEngine,
    ) -> Self {
        let schema = vec![SchemaEntry {
            name: "rows_affected".to_string(),
            table: None,
        }];
        Update {
            table,
            input,
            assignments,
            schema,
            result: None,
            done: false,
            engine,
        }
    }
}

impl PhysicalOperator for Update {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self) -> Result<()> {
        let eng = unsafe { &mut *self.engine };
        let table_schema = eng.catalog.get_table(&self.table)?.clone();

        // Collect all rows first to avoid borrow issues
        self.input.open()?;
        let input_schema = self.input.schema().to_vec();
        let mut to_update: Vec<(RowId, Row)> = Vec::new();

        // The input may be a SeqScan or IndexScan which exposes row_ids
        // We need to get row_ids; we'll use a workaround: scan all rows
        // and get their row_ids from the engine.
        // Actually we need to expose row_ids from the scan operators.
        // For simplicity, we collect rows from input and use scan_table
        // to find matching row_ids.
        let mut input_rows = Vec::new();
        while let Some(row) = self.input.next()? {
            input_rows.push(row);
        }
        self.input.close()?;

        // Now re-scan the table to get row_ids for matching rows
        let all_rows = eng.scan_table(&self.table)?;
        let mut updated = 0i64;

        for (rid, current_row) in all_rows {
            // Check if this row was in the input set (by value comparison)
            if !input_rows.iter().any(|ir| ir == &current_row) {
                continue;
            }

            let ctx = EvalContext::from_row(&input_schema, &current_row);
            let mut new_row = current_row.clone();

            for (col_name, expr) in &self.assignments {
                let idx = table_schema
                    .col_index(col_name)
                    .ok_or_else(|| crate::error::MiniDbError::ColumnNotFound(col_name.clone()))?;
                let val = eval_expr(expr, &ctx)?;
                if idx < new_row.len() {
                    new_row[idx] = if val.is_null() {
                        val
                    } else {
                        val.coerce_to(&table_schema.columns[idx].data_type)?
                    };
                }
            }

            eng.update_row(&self.table, rid, &new_row)?;
            updated += 1;
        }

        self.result = Some(vec![Value::Int(updated)]);
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

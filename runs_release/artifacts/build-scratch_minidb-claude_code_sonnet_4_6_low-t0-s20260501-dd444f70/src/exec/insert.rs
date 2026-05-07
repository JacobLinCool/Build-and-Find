use crate::catalog::TableSchema;
use crate::error::{MiniDbError, Result};
use crate::exec::filter::eval_expr;
use crate::exec::{EvalContext, PhysicalOperator, Schema, SchemaEntry};
use crate::parser::ast::Expr;
use crate::storage::StorageEngine;
use crate::types::{Row, Value};

pub struct Insert {
    pub table: String,
    pub columns: Option<Vec<String>>,
    pub rows: Vec<Vec<Expr>>,
    schema: Schema,
    result: Option<Row>,
    done: bool,
    engine: *mut StorageEngine,
}

impl Insert {
    pub fn new(
        table: String,
        columns: Option<Vec<String>>,
        rows: Vec<Vec<Expr>>,
        engine: *mut StorageEngine,
    ) -> Self {
        let schema = vec![SchemaEntry {
            name: "rows_inserted".to_string(),
            table: None,
        }];
        Insert {
            table,
            columns,
            rows,
            schema,
            result: None,
            done: false,
            engine,
        }
    }
}

impl PhysicalOperator for Insert {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self) -> Result<()> {
        let eng = unsafe { &mut *self.engine };
        let table_schema = eng.catalog.get_table(&self.table)?.clone();

        let mut inserted = 0i64;
        let ctx = EvalContext::default();

        for row_exprs in &self.rows {
            let full_row = build_insert_row(row_exprs, &self.columns, &table_schema, &ctx)?;

            // Check NOT NULL constraints
            for (i, col) in table_schema.columns.iter().enumerate() {
                let val = full_row.get(i).unwrap_or(&Value::Null);
                if val.is_null() && !col.nullable {
                    return Err(MiniDbError::Constraint(format!(
                        "column '{}' is NOT NULL",
                        col.name
                    )));
                }
            }

            eng.insert_row(&self.table, &full_row)?;
            inserted += 1;
        }

        self.result = Some(vec![Value::Int(inserted)]);
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

fn build_insert_row(
    exprs: &[Expr],
    columns: &Option<Vec<String>>,
    schema: &TableSchema,
    ctx: &EvalContext,
) -> Result<Row> {
    let ncols = schema.columns.len();
    let mut row = vec![Value::Null; ncols];

    match columns {
        None => {
            // Positional
            if exprs.len() != ncols {
                return Err(MiniDbError::Runtime(format!(
                    "expected {} values, got {}",
                    ncols,
                    exprs.len()
                )));
            }
            for (i, expr) in exprs.iter().enumerate() {
                row[i] = eval_expr(expr, ctx)?;
            }
        }
        Some(col_names) => {
            // Named columns - first apply defaults
            for (i, col) in schema.columns.iter().enumerate() {
                if let Some(ref default) = col.default {
                    row[i] = default.clone();
                }
            }
            // Then apply specified values
            for (col_name, expr) in col_names.iter().zip(exprs.iter()) {
                let idx = schema
                    .col_index(col_name)
                    .ok_or_else(|| MiniDbError::ColumnNotFound(col_name.clone()))?;
                row[idx] = eval_expr(expr, ctx)?;
            }
        }
    }

    // Type coercion
    for (i, col) in schema.columns.iter().enumerate() {
        if !row[i].is_null() {
            row[i] = row[i].coerce_to(&col.data_type)?;
        }
    }

    Ok(row)
}

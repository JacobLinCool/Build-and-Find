use crate::error::Result;
use crate::exec::{EvalContext, PhysicalOperator, Schema, SchemaEntry};
use crate::plan::physical::IndexPredicate;
use crate::storage::StorageEngine;
use crate::types::{Row, RowId, Value};

pub struct IndexScan {
    pub table: String,
    pub alias: Option<String>,
    pub index_name: String,
    pub predicate: IndexPredicate,
    schema: Schema,
    rows: Vec<(RowId, Row)>,
    cursor: usize,
    engine: *mut StorageEngine,
}

impl IndexScan {
    pub fn new(
        table: String,
        alias: Option<String>,
        index_name: String,
        predicate: IndexPredicate,
        engine: *mut StorageEngine,
    ) -> Result<Self> {
        let eng = unsafe { &mut *engine };
        let ts = eng.catalog.get_table(&table)?;
        let schema = ts
            .columns
            .iter()
            .map(|c| SchemaEntry {
                name: c.name.clone(),
                table: Some(alias.clone().unwrap_or_else(|| table.clone())),
            })
            .collect();
        Ok(IndexScan {
            table,
            alias,
            index_name,
            predicate,
            schema,
            rows: Vec::new(),
            cursor: 0,
            engine,
        })
    }
}

impl PhysicalOperator for IndexScan {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self) -> Result<()> {
        let eng = unsafe { &mut *self.engine };

        // Evaluate index predicate to get row_ids
        let row_ids = match &self.predicate {
            IndexPredicate::Eq(expr) => {
                let key = eval_literal(expr);
                eng.index_lookup_eq(&self.index_name, &key)
            }
            IndexPredicate::Range {
                low,
                low_inclusive,
                high,
                high_inclusive,
            } => {
                let low_val = low.as_ref().map(eval_literal);
                let high_val = high.as_ref().map(eval_literal);
                eng.index_lookup_range(
                    &self.index_name,
                    low_val.as_ref(),
                    *low_inclusive,
                    high_val.as_ref(),
                    *high_inclusive,
                )
            }
        };

        // Fetch rows
        let mut rows = Vec::new();
        for rid in row_ids {
            if let Some(row) = eng.get_row(&self.table, rid)? {
                rows.push((rid, row));
            }
        }
        self.rows = rows;
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

impl IndexScan {
    pub fn next_with_id(&mut self) -> Result<Option<(RowId, Row)>> {
        if self.cursor >= self.rows.len() {
            return Ok(None);
        }
        let item = self.rows[self.cursor].clone();
        self.cursor += 1;
        Ok(Some(item))
    }
}

fn eval_literal(expr: &crate::parser::ast::Expr) -> Value {
    match expr {
        crate::parser::ast::Expr::Lit(v) => v.clone(),
        _ => Value::Null,
    }
}

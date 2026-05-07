use crate::error::Result;
use crate::exec::filter::eval_expr;
use crate::exec::{EvalContext, PhysicalOperator, Schema};
use crate::parser::ast::OrderByItem;
use crate::types::Row;
use std::cmp::Ordering;

pub struct Sort {
    pub input: Box<dyn PhysicalOperator>,
    pub keys: Vec<OrderByItem>,
    sorted_rows: Vec<Row>,
    cursor: usize,
}

impl Sort {
    pub fn new(input: Box<dyn PhysicalOperator>, keys: Vec<OrderByItem>) -> Self {
        Sort {
            input,
            keys,
            sorted_rows: Vec::new(),
            cursor: 0,
        }
    }
}

impl PhysicalOperator for Sort {
    fn schema(&self) -> &Schema {
        self.input.schema()
    }

    fn open(&mut self) -> Result<()> {
        self.input.open()?;
        let mut rows = Vec::new();
        while let Some(row) = self.input.next()? {
            rows.push(row);
        }

        let schema = self.input.schema().to_vec();
        let keys = &self.keys;

        let mut errors: Vec<crate::error::MiniDbError> = Vec::new();
        rows.sort_by(|a, b| {
            let ctx_a = EvalContext::from_row(&schema, a);
            let ctx_b = EvalContext::from_row(&schema, b);
            for key in keys {
                let va = eval_expr(&key.expr, &ctx_a);
                let vb = eval_expr(&key.expr, &ctx_b);
                match (va, vb) {
                    (Ok(va), Ok(vb)) => {
                        let ord = va.sql_cmp(&vb).unwrap_or(Ordering::Equal);
                        let ord = if key.asc { ord } else { ord.reverse() };
                        if ord != Ordering::Equal {
                            return ord;
                        }
                    }
                    _ => {}
                }
            }
            Ordering::Equal
        });

        self.sorted_rows = rows;
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.cursor >= self.sorted_rows.len() {
            return Ok(None);
        }
        let row = self.sorted_rows[self.cursor].clone();
        self.cursor += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        self.sorted_rows.clear();
        self.input.close()
    }
}

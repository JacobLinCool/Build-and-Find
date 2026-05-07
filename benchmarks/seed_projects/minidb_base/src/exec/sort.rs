//! Full in-memory sort. Consumes the entire child stream during `open`,
//! then hands rows out of the sorted buffer.

use super::{eval_expr, ExecContext, PhysicalOperator};
use crate::error::Result;
use crate::parser::ast::OrderTerm;
use crate::plan::PlanSchema;
use crate::types::{compare_values, Row, Schema};

pub struct Sort {
    order_by: Vec<OrderTerm>,
    child: Box<dyn PhysicalOperator>,
    child_schema: PlanSchema,
    buffer: Vec<Row>,
    cursor: usize,
}

impl Sort {
    pub fn new(
        order_by: Vec<OrderTerm>,
        child: Box<dyn PhysicalOperator>,
        child_schema: PlanSchema,
    ) -> Self {
        Self {
            order_by,
            child,
            child_schema,
            buffer: Vec::new(),
            cursor: 0,
        }
    }
}

impl PhysicalOperator for Sort {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.open(ctx)?;
        while let Some(r) = self.child.next(ctx)? {
            self.buffer.push(r);
        }
        self.child.close(ctx)?;

        // Materialise keys first to avoid re-evaluating per comparison.
        let terms = self.order_by.clone();
        let schema = self.child_schema.clone();
        let mut keyed: Vec<(Vec<crate::types::Value>, Row)> = Vec::with_capacity(self.buffer.len());
        for row in self.buffer.drain(..) {
            let mut keys = Vec::with_capacity(terms.len());
            for t in &terms {
                keys.push(eval_expr(&t.expr, &row, &schema)?);
            }
            keyed.push((keys, row));
        }
        keyed.sort_by(|a, b| {
            for (i, term) in terms.iter().enumerate() {
                let av = &a.0[i];
                let bv = &b.0[i];
                let ord = compare_values(av, bv).unwrap_or(std::cmp::Ordering::Equal);
                let ord = if term.ascending { ord } else { ord.reverse() };
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            std::cmp::Ordering::Equal
        });
        self.buffer = keyed.into_iter().map(|(_, r)| r).collect();
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
        &self.child_schema.schema
    }
}

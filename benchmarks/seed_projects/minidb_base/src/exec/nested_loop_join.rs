//! Textbook nested-loop INNER JOIN. The outer relation drives, the inner
//! is re-scanned for every outer row. This is quadratic, but perfectly
//! adequate for the seed's workload.

use super::{is_true, ExecContext, PhysicalOperator};
use crate::error::Result;
use crate::parser::ast::Expression;
use crate::plan::PlanSchema;
use crate::types::{Row, Schema};

pub struct NestedLoopJoin {
    left: Box<dyn PhysicalOperator>,
    right: Box<dyn PhysicalOperator>,
    on: Expression,
    out_schema: PlanSchema,
    combined_schema: PlanSchema,

    left_buf: Vec<Row>,
    right_buf: Vec<Row>,
    outer_idx: usize,
    inner_idx: usize,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        on: Expression,
        out_schema: PlanSchema,
        combined_schema: PlanSchema,
    ) -> Self {
        Self {
            left,
            right,
            on,
            out_schema,
            combined_schema,
            left_buf: Vec::new(),
            right_buf: Vec::new(),
            outer_idx: 0,
            inner_idx: 0,
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.left.open(ctx)?;
        while let Some(r) = self.left.next(ctx)? {
            self.left_buf.push(r);
        }
        self.left.close(ctx)?;

        self.right.open(ctx)?;
        while let Some(r) = self.right.next(ctx)? {
            self.right_buf.push(r);
        }
        self.right.close(ctx)?;

        self.outer_idx = 0;
        self.inner_idx = 0;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecContext<'_>) -> Result<Option<Row>> {
        while self.outer_idx < self.left_buf.len() {
            while self.inner_idx < self.right_buf.len() {
                let mut combined = self.left_buf[self.outer_idx].clone();
                combined.extend(self.right_buf[self.inner_idx].clone());
                self.inner_idx += 1;
                if is_true(&self.on, &combined, &self.combined_schema)? {
                    return Ok(Some(combined));
                }
            }
            self.outer_idx += 1;
            self.inner_idx = 0;
        }
        Ok(None)
    }

    fn close(&mut self, _ctx: &mut ExecContext<'_>) -> Result<()> {
        self.left_buf.clear();
        self.right_buf.clear();
        Ok(())
    }

    fn schema(&self) -> &Schema {
        &self.out_schema.schema
    }
}

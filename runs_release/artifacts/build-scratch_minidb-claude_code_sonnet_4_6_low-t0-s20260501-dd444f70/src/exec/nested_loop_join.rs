use crate::error::Result;
use crate::exec::filter::eval_expr;
use crate::exec::{EvalContext, PhysicalOperator, Schema, SchemaEntry};
use crate::parser::ast::Expr;
use crate::types::{Row, Value};

pub struct NestedLoopJoin {
    pub left: Box<dyn PhysicalOperator>,
    pub right: Box<dyn PhysicalOperator>,
    pub condition: Expr,
    schema: Schema,
    left_rows: Vec<Row>,
    left_idx: usize,
    right_rows: Vec<Row>,
    right_idx: usize,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        condition: Expr,
    ) -> Self {
        let mut schema = Vec::new();
        schema.extend(left.schema().iter().cloned());
        schema.extend(right.schema().iter().cloned());
        NestedLoopJoin {
            left,
            right,
            condition,
            schema,
            left_rows: Vec::new(),
            left_idx: 0,
            right_rows: Vec::new(),
            right_idx: 0,
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self) -> Result<()> {
        self.left.open()?;
        let mut left_rows = Vec::new();
        while let Some(row) = self.left.next()? {
            left_rows.push(row);
        }
        self.left.close()?;
        self.left_rows = left_rows;
        self.left_idx = 0;

        self.right.open()?;
        let mut right_rows = Vec::new();
        while let Some(row) = self.right.next()? {
            right_rows.push(row);
        }
        self.right.close()?;
        self.right_rows = right_rows;
        self.right_idx = 0;

        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        let left_schema = &self.left.schema().to_vec();
        let right_schema = &self.right.schema().to_vec();

        while self.left_idx < self.left_rows.len() {
            let left_row = &self.left_rows[self.left_idx];

            while self.right_idx < self.right_rows.len() {
                let right_row = &self.right_rows[self.right_idx];
                self.right_idx += 1;

                // Build combined context
                let mut ctx = EvalContext::from_row(left_schema, left_row);
                let right_ctx = EvalContext::from_row(right_schema, right_row);
                ctx.merge(&right_ctx);

                let result = eval_expr(&self.condition, &ctx)?;
                if result.is_truthy().is_true() {
                    let mut combined = left_row.clone();
                    combined.extend(right_row.iter().cloned());
                    return Ok(Some(combined));
                }
            }

            self.left_idx += 1;
            self.right_idx = 0;
        }

        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        self.left_rows.clear();
        self.right_rows.clear();
        Ok(())
    }
}

use super::expr::eval;
use super::operator::{ExecSchema, PhysicalOperator, Tuple};
use crate::error::Result;
use crate::parser::ast::Expr;
use crate::types::Value;

/// Volcano nested-loop join. Materializes the right side once, then iterates
/// the left side in the outer loop.
pub struct NestedLoopJoin {
    left: Box<dyn PhysicalOperator>,
    right: Box<dyn PhysicalOperator>,
    on: Expr,
    right_rows: Vec<Tuple>,
    output_schema: ExecSchema,
    cur_left: Option<Tuple>,
    right_cursor: usize,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        on: Expr,
    ) -> Self {
        let mut output = left.schema().columns.clone();
        output.extend(right.schema().columns.clone());
        NestedLoopJoin {
            left,
            right,
            on,
            right_rows: Vec::new(),
            output_schema: ExecSchema { columns: output },
            cur_left: None,
            right_cursor: 0,
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self) -> Result<()> {
        self.left.open()?;
        self.right.open()?;
        // materialize right side
        self.right_rows.clear();
        while let Some(r) = self.right.next()? {
            self.right_rows.push(r);
        }
        self.cur_left = self.left.next()?;
        self.right_cursor = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Tuple>> {
        loop {
            let Some(left_row) = self.cur_left.clone() else {
                return Ok(None);
            };
            if self.right_cursor >= self.right_rows.len() {
                self.cur_left = self.left.next()?;
                self.right_cursor = 0;
                continue;
            }
            let right_row = &self.right_rows[self.right_cursor];
            self.right_cursor += 1;
            let mut combined = left_row.clone();
            combined.extend_from_slice(right_row);
            let v = eval(&self.on, &self.output_schema, &combined)?;
            let ok = match v {
                Value::Bool(b) => b,
                Value::Null => false,
                other => other.truthy().unwrap_or(false),
            };
            if ok {
                return Ok(Some(combined));
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        self.left.close()?;
        self.right.close()
    }

    fn schema(&self) -> &ExecSchema {
        &self.output_schema
    }
}

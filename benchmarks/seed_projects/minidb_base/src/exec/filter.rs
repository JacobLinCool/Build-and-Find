//! Row-at-a-time filter operator.

use super::{is_true, ExecContext, PhysicalOperator};
use crate::error::Result;
use crate::parser::ast::Expression;
use crate::plan::PlanSchema;
use crate::types::{Row, Schema};

pub struct Filter {
    predicate: Expression,
    child: Box<dyn PhysicalOperator>,
    child_schema: PlanSchema,
    out_schema: PlanSchema,
}

impl Filter {
    pub fn new(
        predicate: Expression,
        child: Box<dyn PhysicalOperator>,
        child_schema: PlanSchema,
        out_schema: PlanSchema,
    ) -> Self {
        Self {
            predicate,
            child,
            child_schema,
            out_schema,
        }
    }

    pub fn predicate(&self) -> &Expression {
        &self.predicate
    }
}

impl PhysicalOperator for Filter {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.open(ctx)
    }

    fn next(&mut self, ctx: &mut ExecContext<'_>) -> Result<Option<Row>> {
        while let Some(row) = self.child.next(ctx)? {
            if is_true(&self.predicate, &row, &self.child_schema)? {
                return Ok(Some(row));
            }
        }
        Ok(None)
    }

    fn close(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.close(ctx)
    }

    fn schema(&self) -> &Schema {
        &self.out_schema.schema
    }
}

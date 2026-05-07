//! Projection / rename operator.

use super::{eval_expr, ExecContext, PhysicalOperator};
use crate::error::Result;
use crate::parser::ast::Projection;
use crate::plan::PlanSchema;
use crate::types::{Row, Schema};

pub struct Project {
    projections: Vec<Projection>,
    child: Box<dyn PhysicalOperator>,
    child_schema: PlanSchema,
    out_schema: PlanSchema,
}

impl Project {
    pub fn new(
        projections: Vec<Projection>,
        child: Box<dyn PhysicalOperator>,
        child_schema: PlanSchema,
        out_schema: PlanSchema,
    ) -> Self {
        Self {
            projections,
            child,
            child_schema,
            out_schema,
        }
    }
}

impl PhysicalOperator for Project {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.open(ctx)
    }

    fn next(&mut self, ctx: &mut ExecContext<'_>) -> Result<Option<Row>> {
        let Some(row) = self.child.next(ctx)? else {
            return Ok(None);
        };
        let mut out = Vec::new();
        for p in &self.projections {
            match p {
                Projection::AllColumns => out.extend(row.iter().cloned()),
                Projection::Expr { expr, .. } => {
                    out.push(eval_expr(expr, &row, &self.child_schema)?);
                }
            }
        }
        Ok(Some(out))
    }

    fn close(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.close(ctx)
    }

    fn schema(&self) -> &Schema {
        &self.out_schema.schema
    }
}

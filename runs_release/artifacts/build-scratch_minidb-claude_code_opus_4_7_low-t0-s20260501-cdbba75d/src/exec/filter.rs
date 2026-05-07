use super::expr::eval;
use super::operator::{ExecSchema, PhysicalOperator, Tuple};
use crate::error::Result;
use crate::parser::ast::Expr;
use crate::types::Value;

pub struct Filter {
    pub child: Box<dyn PhysicalOperator>,
    pub predicate: Expr,
}

impl Filter {
    pub fn new(child: Box<dyn PhysicalOperator>, predicate: Expr) -> Self {
        Filter { child, predicate }
    }
}

impl PhysicalOperator for Filter {
    fn open(&mut self) -> Result<()> {
        self.child.open()
    }

    fn next(&mut self) -> Result<Option<Tuple>> {
        let schema = self.child.schema().clone();
        loop {
            let Some(row) = self.child.next()? else {
                return Ok(None);
            };
            let v = eval(&self.predicate, &schema, &row)?;
            // SQL semantics: NULL is treated as not-matching for WHERE.
            let pass = match v {
                Value::Bool(b) => b,
                Value::Null => false,
                other => other.truthy().unwrap_or(false),
            };
            if pass {
                return Ok(Some(row));
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        self.child.close()
    }

    fn schema(&self) -> &ExecSchema {
        self.child.schema()
    }
}

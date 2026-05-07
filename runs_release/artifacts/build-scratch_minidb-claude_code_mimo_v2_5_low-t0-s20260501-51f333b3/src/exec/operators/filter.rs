use super::PhysicalOperator;
use crate::error::Result;
use crate::exec::evaluator::ExprEvaluator;
use crate::parser::ast::Expr;
use crate::types::Row;

pub struct Filter<'a> {
    input: Box<dyn PhysicalOperator + 'a>,
    predicate: Expr,
    evaluator: ExprEvaluator,
}

impl<'a> Filter<'a> {
    pub fn new(
        input: Box<dyn PhysicalOperator + 'a>,
        predicate: Expr,
        schema_names: Vec<String>,
    ) -> Self {
        Filter {
            input,
            predicate: predicate.clone(),
            evaluator: ExprEvaluator::new(predicate, schema_names),
        }
    }
}

impl<'a> PhysicalOperator for Filter<'a> {
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }

    fn next(&mut self) -> Result<Option<Row>> {
        while let Some(row) = self.input.next()? {
            let result = self.evaluator.evaluate(&row)?;
            match result {
                crate::types::Value::Bool(true) => return Ok(Some(row)),
                crate::types::Value::Bool(false) => continue,
                crate::types::Value::Null => continue,
                _ => continue,
            }
        }
        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }

    fn schema(&self) -> Vec<String> {
        self.input.schema()
    }
}

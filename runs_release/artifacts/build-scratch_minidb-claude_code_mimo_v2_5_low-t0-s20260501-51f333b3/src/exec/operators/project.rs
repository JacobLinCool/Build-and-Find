use super::PhysicalOperator;
use crate::error::{MiniDbError, Result};
use crate::exec::evaluator::ExprEvaluator;
use crate::parser::ast::{Expr, SelectColumn};
use crate::types::{Row, Value};

pub struct Project<'a> {
    input: Box<dyn PhysicalOperator + 'a>,
    columns: Vec<SelectColumn>,
    evaluators: Vec<(ExprEvaluator, String)>,
}

impl<'a> Project<'a> {
    pub fn new(
        input: Box<dyn PhysicalOperator + 'a>,
        columns: Vec<SelectColumn>,
        input_schema: Vec<String>,
    ) -> Self {
        let mut evaluators = Vec::new();
        for col in &columns {
            match col {
                SelectColumn::Star => {
                    // Star is handled specially
                }
                SelectColumn::Expr(expr, alias) => {
                    let name = alias.clone().unwrap_or_else(|| format!("{:?}", expr));
                    evaluators.push((ExprEvaluator::new(expr.clone(), input_schema.clone()), name));
                }
            }
        }

        Project {
            input,
            columns,
            evaluators,
        }
    }
}

impl<'a> PhysicalOperator for Project<'a> {
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }

    fn next(&mut self) -> Result<Option<Row>> {
        let row = match self.input.next()? {
            Some(r) => r,
            None => return Ok(None),
        };

        // Check if we have star projection
        let has_star = self.columns.iter().any(|c| matches!(c, SelectColumn::Star));

        if has_star {
            return Ok(Some(row));
        }

        let mut values = Vec::new();
        for (evaluator, _) in &self.evaluators {
            let val = evaluator.evaluate(&row)?;
            values.push(val);
        }

        Ok(Some(Row::new(values)))
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }

    fn schema(&self) -> Vec<String> {
        let has_star = self.columns.iter().any(|c| matches!(c, SelectColumn::Star));

        if has_star {
            return self.input.schema();
        }

        self.evaluators
            .iter()
            .map(|(_, name)| name.clone())
            .collect()
    }
}

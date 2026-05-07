use super::PhysicalOperator;
use crate::error::Result;
use crate::exec::evaluator::ExprEvaluator;
use crate::parser::ast::OrderByExpr;
use crate::types::{Row, Value};

pub struct Sort<'a> {
    input: Box<dyn PhysicalOperator + 'a>,
    order_by: Vec<OrderByExpr>,
    rows: Vec<Row>,
    pos: usize,
    schema_names: Vec<String>,
}

impl<'a> Sort<'a> {
    pub fn new(
        input: Box<dyn PhysicalOperator + 'a>,
        order_by: Vec<OrderByExpr>,
        schema_names: Vec<String>,
    ) -> Self {
        Sort {
            input,
            order_by,
            rows: Vec::new(),
            pos: 0,
            schema_names,
        }
    }
}

impl<'a> PhysicalOperator for Sort<'a> {
    fn open(&mut self) -> Result<()> {
        self.input.open()?;
        self.rows.clear();
        self.pos = 0;

        // Collect all rows
        while let Some(row) = self.input.next()? {
            self.rows.push(row);
        }

        // Sort
        let order_by = self.order_by.clone();
        let schema = self.schema_names.clone();
        self.rows.sort_by(|a, b| {
            for ob in &order_by {
                let evaluator = ExprEvaluator::new(ob.expr.clone(), schema.clone());
                let val_a = evaluator.evaluate(a).unwrap_or(Value::Null);
                let val_b = evaluator.evaluate(b).unwrap_or(Value::Null);

                let cmp = compare_values(&val_a, &val_b);
                let ordered = if ob.ascending { cmp } else { cmp.reverse() };

                if ordered != std::cmp::Ordering::Equal {
                    return ordered;
                }
            }
            std::cmp::Ordering::Equal
        });

        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos >= self.rows.len() {
            return Ok(None);
        }
        let row = self.rows[self.pos].clone();
        self.pos += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }

    fn schema(&self) -> Vec<String> {
        self.input.schema()
    }
}

fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
        (Value::Null, _) => std::cmp::Ordering::Less,
        (_, Value::Null) => std::cmp::Ordering::Greater,
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Int(a), Value::Real(b)) => (*a as f64)
            .partial_cmp(b)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Value::Real(a), Value::Int(b)) => a
            .partial_cmp(&(*b as f64))
            .unwrap_or(std::cmp::Ordering::Equal),
        (Value::Real(a), Value::Real(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Text(a), Value::Text(b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        _ => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_values() {
        assert_eq!(
            compare_values(&Value::Int(1), &Value::Int(2)),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_values(&Value::Int(2), &Value::Int(1)),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_values(&Value::Int(1), &Value::Int(1)),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            compare_values(&Value::Null, &Value::Int(1)),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_values(&Value::Int(1), &Value::Null),
            std::cmp::Ordering::Greater
        );
    }
}

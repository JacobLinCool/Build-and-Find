use crate::error::{MiniDbError, Result};
use crate::parser::ast::{BinOp, Expr, UnOp};
use crate::types::{sql_and, sql_not, sql_or, Row, Value};

pub struct ExprEvaluator {
    expr: Expr,
    schema: Vec<String>,
}

impl ExprEvaluator {
    pub fn new(expr: Expr, schema: Vec<String>) -> Self {
        ExprEvaluator { expr, schema }
    }

    pub fn evaluate(&self, row: &Row) -> Result<Value> {
        self.eval_expr(&self.expr, row)
    }

    pub fn evaluate_with_row(&self, expr: &Expr, row: &Row) -> Result<Value> {
        self.eval_expr(expr, row)
    }

    fn eval_expr(&self, expr: &Expr, row: &Row) -> Result<Value> {
        match expr {
            Expr::Literal(lit) => Ok(lit.to_value()),
            Expr::Column(col_ref) => {
                let idx = if let Some(ref table) = col_ref.table {
                    // For qualified references, we need to handle table prefix
                    // For now, just use the column name
                    self.schema.iter().position(|s| s == &col_ref.name)
                } else {
                    self.schema.iter().position(|s| s == &col_ref.name)
                };

                match idx {
                    Some(i) => Ok(row.values.get(i).cloned().unwrap_or(Value::Null)),
                    None => Err(MiniDbError::SqlError(format!(
                        "column '{}' not found",
                        col_ref.name
                    ))),
                }
            }
            Expr::BinaryOp { left, op, right } => {
                let l = self.eval_expr(left, row)?;
                let r = self.eval_expr(right, row)?;
                self.eval_binop(op, &l, &r)
            }
            Expr::UnaryOp { op, expr } => {
                let val = self.eval_expr(expr, row)?;
                self.eval_unop(op, &val)
            }
            Expr::IsNull { expr, negated } => {
                let val = self.eval_expr(expr, row)?;
                let is_null = val.is_null();
                if *negated {
                    Ok(Value::Bool(!is_null))
                } else {
                    Ok(Value::Bool(is_null))
                }
            }
            Expr::Function { name, args } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(arg, row)?);
                }
                self.eval_function(name, &arg_vals)
            }
        }
    }

    fn eval_binop(&self, op: &BinOp, l: &Value, r: &Value) -> Result<Value> {
        // Handle NULL propagation for comparisons
        match op {
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                if l.is_null() || r.is_null() {
                    return Ok(Value::Null);
                }
            }
            BinOp::And => {
                let l_bool = match l {
                    Value::Bool(b) => Some(*b),
                    Value::Null => None,
                    _ => None,
                };
                let r_bool = match r {
                    Value::Bool(b) => Some(*b),
                    Value::Null => None,
                    _ => None,
                };
                return match sql_and(l_bool, r_bool) {
                    Some(b) => Ok(Value::Bool(b)),
                    None => Ok(Value::Null),
                };
            }
            BinOp::Or => {
                let l_bool = match l {
                    Value::Bool(b) => Some(*b),
                    Value::Null => None,
                    _ => None,
                };
                let r_bool = match r {
                    Value::Bool(b) => Some(*b),
                    Value::Null => None,
                    _ => None,
                };
                return match sql_or(l_bool, r_bool) {
                    Some(b) => Ok(Value::Bool(b)),
                    None => Ok(Value::Null),
                };
            }
            _ => {}
        }

        // Arithmetic operations
        match op {
            BinOp::Add => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a + b)),
                (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 + b)),
                (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a + *b as f64)),
                _ => Err(MiniDbError::TypeError {
                    message: "unsupported operands for +".to_string(),
                }),
            },
            BinOp::Sub => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
                (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a - b)),
                (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 - b)),
                (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a - *b as f64)),
                _ => Err(MiniDbError::TypeError {
                    message: "unsupported operands for -".to_string(),
                }),
            },
            BinOp::Mul => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
                (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a * b)),
                (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 * b)),
                (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a * *b as f64)),
                _ => Err(MiniDbError::TypeError {
                    message: "unsupported operands for *".to_string(),
                }),
            },
            BinOp::Div => match (l, r) {
                (Value::Int(a), Value::Int(b)) => {
                    if *b == 0 {
                        Err(MiniDbError::SqlError("division by zero".to_string()))
                    } else {
                        Ok(Value::Int(a / b))
                    }
                }
                (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a / b)),
                (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 / b)),
                (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a / *b as f64)),
                _ => Err(MiniDbError::TypeError {
                    message: "unsupported operands for /".to_string(),
                }),
            },
            BinOp::Eq => Ok(Value::Bool(l == r)),
            BinOp::Ne => Ok(Value::Bool(l != r)),
            BinOp::Lt => Ok(Value::Bool(
                compare_values(l, r) == std::cmp::Ordering::Less,
            )),
            BinOp::Le => Ok(Value::Bool(matches!(
                compare_values(l, r),
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal
            ))),
            BinOp::Gt => Ok(Value::Bool(
                compare_values(l, r) == std::cmp::Ordering::Greater,
            )),
            BinOp::Ge => Ok(Value::Bool(matches!(
                compare_values(l, r),
                std::cmp::Ordering::Greater | std::cmp::Ordering::Equal
            ))),
            BinOp::Concat => {
                let ls = match l {
                    Value::Text(s) => s.clone(),
                    _ => l.to_string(),
                };
                let rs = match r {
                    Value::Text(s) => s.clone(),
                    _ => r.to_string(),
                };
                Ok(Value::Text(format!("{}{}", ls, rs)))
            }
            _ => Err(MiniDbError::TypeError {
                message: "unsupported binary operator".to_string(),
            }),
        }
    }

    fn eval_unop(&self, op: &UnOp, val: &Value) -> Result<Value> {
        match op {
            UnOp::Neg => match val {
                Value::Int(v) => Ok(Value::Int(-v)),
                Value::Real(v) => Ok(Value::Real(-v)),
                Value::Null => Ok(Value::Null),
                _ => Err(MiniDbError::TypeError {
                    message: "cannot negate non-numeric value".to_string(),
                }),
            },
            UnOp::Not => match val {
                Value::Bool(v) => Ok(Value::Bool(!v)),
                Value::Null => Ok(Value::Null),
                _ => Err(MiniDbError::TypeError {
                    message: "cannot apply NOT to non-boolean value".to_string(),
                }),
            },
        }
    }

    fn eval_function(&self, name: &str, args: &[Value]) -> Result<Value> {
        match name {
            "LENGTH" => {
                if args.len() != 1 {
                    return Err(MiniDbError::SqlError(
                        "LENGTH takes exactly one argument".to_string(),
                    ));
                }
                match &args[0] {
                    Value::Text(s) => Ok(Value::Int(s.len() as i64)),
                    Value::Null => Ok(Value::Null),
                    _ => Err(MiniDbError::TypeError {
                        message: "LENGTH requires a string argument".to_string(),
                    }),
                }
            }
            "LOWER" => {
                if args.len() != 1 {
                    return Err(MiniDbError::SqlError(
                        "LOWER takes exactly one argument".to_string(),
                    ));
                }
                match &args[0] {
                    Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
                    Value::Null => Ok(Value::Null),
                    _ => Err(MiniDbError::TypeError {
                        message: "LOWER requires a string argument".to_string(),
                    }),
                }
            }
            "UPPER" => {
                if args.len() != 1 {
                    return Err(MiniDbError::SqlError(
                        "UPPER takes exactly one argument".to_string(),
                    ));
                }
                match &args[0] {
                    Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
                    Value::Null => Ok(Value::Null),
                    _ => Err(MiniDbError::TypeError {
                        message: "UPPER requires a string argument".to_string(),
                    }),
                }
            }
            "SUBSTR" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(MiniDbError::SqlError(
                        "SUBSTR takes 2 or 3 arguments".to_string(),
                    ));
                }
                let s = match &args[0] {
                    Value::Text(s) => s,
                    Value::Null => return Ok(Value::Null),
                    _ => {
                        return Err(MiniDbError::TypeError {
                            message: "SUBSTR requires a string argument".to_string(),
                        })
                    }
                };
                let start = match &args[1] {
                    Value::Int(v) => *v as usize,
                    _ => {
                        return Err(MiniDbError::TypeError {
                            message: "SUBSTR start must be integer".to_string(),
                        })
                    }
                };
                let len = if args.len() == 3 {
                    match &args[2] {
                        Value::Int(v) => *v as usize,
                        _ => {
                            return Err(MiniDbError::TypeError {
                                message: "SUBSTR length must be integer".to_string(),
                            })
                        }
                    }
                } else {
                    s.len()
                };
                let start = start.saturating_sub(1); // SQL is 1-indexed
                let end = (start + len).min(s.len());
                if start >= s.len() {
                    Ok(Value::Text(String::new()))
                } else {
                    Ok(Value::Text(s[start..end].to_string()))
                }
            }
            "ABS" => {
                if args.len() != 1 {
                    return Err(MiniDbError::SqlError(
                        "ABS takes exactly one argument".to_string(),
                    ));
                }
                match &args[0] {
                    Value::Int(v) => Ok(Value::Int(v.abs())),
                    Value::Real(v) => Ok(Value::Real(v.abs())),
                    Value::Null => Ok(Value::Null),
                    _ => Err(MiniDbError::TypeError {
                        message: "ABS requires a numeric argument".to_string(),
                    }),
                }
            }
            "COALESCE" => {
                for arg in args {
                    if !arg.is_null() {
                        return Ok(arg.clone());
                    }
                }
                Ok(Value::Null)
            }
            _ => Err(MiniDbError::SqlError(format!("unknown function: {}", name))),
        }
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
    use crate::types::{ColumnRef, SqlLiteral};

    #[test]
    fn test_literal_eval() {
        let evaluator = ExprEvaluator::new(Expr::Literal(SqlLiteral::Int(42)), vec![]);
        let row = Row::new(vec![]);
        assert_eq!(evaluator.evaluate(&row).unwrap(), Value::Int(42));
    }

    #[test]
    fn test_column_eval() {
        let evaluator = ExprEvaluator::new(
            Expr::Column(ColumnRef::new("name".to_string())),
            vec!["name".to_string()],
        );
        let row = Row::new(vec![Value::Text("Alice".to_string())]);
        assert_eq!(
            evaluator.evaluate(&row).unwrap(),
            Value::Text("Alice".to_string())
        );
    }

    #[test]
    fn test_addition() {
        let evaluator = ExprEvaluator::new(
            Expr::BinaryOp {
                left: Box::new(Expr::Literal(SqlLiteral::Int(1))),
                op: BinOp::Add,
                right: Box::new(Expr::Literal(SqlLiteral::Int(2))),
            },
            vec![],
        );
        let row = Row::new(vec![]);
        assert_eq!(evaluator.evaluate(&row).unwrap(), Value::Int(3));
    }

    #[test]
    fn test_comparison() {
        let evaluator = ExprEvaluator::new(
            Expr::BinaryOp {
                left: Box::new(Expr::Literal(SqlLiteral::Int(1))),
                op: BinOp::Lt,
                right: Box::new(Expr::Literal(SqlLiteral::Int(2))),
            },
            vec![],
        );
        let row = Row::new(vec![]);
        assert_eq!(evaluator.evaluate(&row).unwrap(), Value::Bool(true));
    }

    #[test]
    fn test_null_propagation() {
        let evaluator = ExprEvaluator::new(
            Expr::BinaryOp {
                left: Box::new(Expr::Literal(SqlLiteral::Null)),
                op: BinOp::Eq,
                right: Box::new(Expr::Literal(SqlLiteral::Int(1))),
            },
            vec![],
        );
        let row = Row::new(vec![]);
        assert_eq!(evaluator.evaluate(&row).unwrap(), Value::Null);
    }

    #[test]
    fn test_function_length() {
        let evaluator = ExprEvaluator::new(
            Expr::Function {
                name: "LENGTH".to_string(),
                args: vec![Expr::Literal(SqlLiteral::Text("hello".to_string()))],
            },
            vec![],
        );
        let row = Row::new(vec![]);
        assert_eq!(evaluator.evaluate(&row).unwrap(), Value::Int(5));
    }
}

/// Expression evaluator
use crate::error::{MiniDbError, Result};
use crate::parser::ast::{BinOp, Expr, UnaryOp};
use crate::plan::Schema;
use crate::types::{Row, Value};

pub struct Evaluator;

impl Evaluator {
    pub fn eval(expr: &Expr, row: &[Value], schema: &Schema) -> Result<Value> {
        match expr {
            Expr::Literal(v) => Ok(v.clone()),

            Expr::Column(name) => {
                let idx = schema
                    .col_index(name)
                    .ok_or_else(|| MiniDbError::ColumnNotFound(name.clone()))?;
                Ok(row.get(idx).cloned().unwrap_or(Value::Null))
            }

            Expr::QualifiedColumn(table, col) => {
                let idx = schema
                    .qualified_col_index(table, col)
                    .or_else(|| schema.col_index(col))
                    .ok_or_else(|| MiniDbError::ColumnNotFound(format!("{}.{}", table, col)))?;
                Ok(row.get(idx).cloned().unwrap_or(Value::Null))
            }

            Expr::BinOp { op, left, right } => Self::eval_binop(op, left, right, row, schema),

            Expr::UnaryOp { op, expr } => {
                let val = Self::eval(expr, row, schema)?;
                match op {
                    UnaryOp::Neg => match val {
                        Value::Int(n) => Ok(Value::Int(-n)),
                        Value::Real(f) => Ok(Value::Real(-f)),
                        Value::Null => Ok(Value::Null),
                        _ => Err(MiniDbError::Type("cannot negate non-numeric".into())),
                    },
                    UnaryOp::Not => match val {
                        Value::Bool(b) => Ok(Value::Bool(!b)),
                        Value::Null => Ok(Value::Null),
                        _ => Err(MiniDbError::Type("cannot NOT non-boolean".into())),
                    },
                }
            }

            Expr::IsNull(expr) => {
                let val = Self::eval(expr, row, schema)?;
                Ok(Value::Bool(val.is_null()))
            }

            Expr::IsNotNull(expr) => {
                let val = Self::eval(expr, row, schema)?;
                Ok(Value::Bool(!val.is_null()))
            }

            Expr::Function { name, args } => Self::eval_function(name, args, row, schema),

            Expr::Between { expr, low, high } => {
                let v = Self::eval(expr, row, schema)?;
                let l = Self::eval(low, row, schema)?;
                let h = Self::eval(high, row, schema)?;
                if v.is_null() || l.is_null() || h.is_null() {
                    return Ok(Value::Null);
                }
                Ok(Value::Bool(v >= l && v <= h))
            }

            Expr::InList {
                expr,
                list,
                negated,
            } => {
                let v = Self::eval(expr, row, schema)?;
                if v.is_null() {
                    return Ok(Value::Null);
                }
                let mut found = false;
                for item in list {
                    let iv = Self::eval(item, row, schema)?;
                    if iv.is_null() {
                        continue;
                    }
                    if v.sql_eq(&iv) == Some(true) {
                        found = true;
                        break;
                    }
                }
                Ok(Value::Bool(if *negated { !found } else { found }))
            }
        }
    }

    fn eval_binop(
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        row: &[Value],
        schema: &Schema,
    ) -> Result<Value> {
        // Short-circuit for AND/OR
        match op {
            BinOp::And => {
                let l = Self::eval(left, row, schema)?;
                match &l {
                    Value::Bool(false) => return Ok(Value::Bool(false)),
                    Value::Null => {
                        let r = Self::eval(right, row, schema)?;
                        return Ok(match r {
                            Value::Bool(false) => Value::Bool(false),
                            _ => Value::Null,
                        });
                    }
                    _ => {}
                }
                let r = Self::eval(right, row, schema)?;
                return Ok(match (&l, &r) {
                    (Value::Bool(true), Value::Bool(true)) => Value::Bool(true),
                    (Value::Bool(false), _) | (_, Value::Bool(false)) => Value::Bool(false),
                    _ => Value::Null,
                });
            }
            BinOp::Or => {
                let l = Self::eval(left, row, schema)?;
                match &l {
                    Value::Bool(true) => return Ok(Value::Bool(true)),
                    Value::Null => {
                        let r = Self::eval(right, row, schema)?;
                        return Ok(match r {
                            Value::Bool(true) => Value::Bool(true),
                            _ => Value::Null,
                        });
                    }
                    _ => {}
                }
                let r = Self::eval(right, row, schema)?;
                return Ok(match (&l, &r) {
                    (Value::Bool(true), _) | (_, Value::Bool(true)) => Value::Bool(true),
                    (Value::Bool(false), Value::Bool(false)) => Value::Bool(false),
                    _ => Value::Null,
                });
            }
            _ => {}
        }

        let l = Self::eval(left, row, schema)?;
        let r = Self::eval(right, row, schema)?;

        match op {
            BinOp::Eq => Ok(match l.sql_eq(&r) {
                Some(b) => Value::Bool(b),
                None => Value::Null,
            }),
            BinOp::Neq => Ok(match l.sql_eq(&r) {
                Some(b) => Value::Bool(!b),
                None => Value::Null,
            }),
            BinOp::Lt => Ok(match l.sql_cmp(&r) {
                Some(std::cmp::Ordering::Less) => Value::Bool(true),
                Some(_) => Value::Bool(false),
                None => Value::Null,
            }),
            BinOp::Lte => Ok(match l.sql_cmp(&r) {
                Some(std::cmp::Ordering::Greater) => Value::Bool(false),
                Some(_) => Value::Bool(true),
                None => Value::Null,
            }),
            BinOp::Gt => Ok(match l.sql_cmp(&r) {
                Some(std::cmp::Ordering::Greater) => Value::Bool(true),
                Some(_) => Value::Bool(false),
                None => Value::Null,
            }),
            BinOp::Gte => Ok(match l.sql_cmp(&r) {
                Some(std::cmp::Ordering::Less) => Value::Bool(false),
                Some(_) => Value::Bool(true),
                None => Value::Null,
            }),
            BinOp::Add => numeric_binop(&l, &r, |a, b| a + b, |a, b| a + b),
            BinOp::Sub => numeric_binop(&l, &r, |a, b| a - b, |a, b| a - b),
            BinOp::Mul => numeric_binop(&l, &r, |a, b| a * b, |a, b| a * b),
            BinOp::Div => {
                if l.is_null() || r.is_null() {
                    return Ok(Value::Null);
                }
                match (&l, &r) {
                    (_, Value::Int(0)) => Err(MiniDbError::Runtime("division by zero".into())),
                    (_, Value::Real(f)) if *f == 0.0 => {
                        Err(MiniDbError::Runtime("division by zero".into()))
                    }
                    (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
                    (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a / b)),
                    (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 / b)),
                    (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a / *b as f64)),
                    _ => Err(MiniDbError::Type("non-numeric operands".into())),
                }
            }
            BinOp::Concat => {
                if l.is_null() || r.is_null() {
                    return Ok(Value::Null);
                }
                let ls = format!("{}", l);
                let rs = format!("{}", r);
                Ok(Value::Text(ls + &rs))
            }
            BinOp::And | BinOp::Or => unreachable!(),
        }
    }

    fn eval_function(name: &str, args: &[Expr], row: &[Value], schema: &Schema) -> Result<Value> {
        let vals: Vec<Value> = args
            .iter()
            .map(|a| Self::eval(a, row, schema))
            .collect::<Result<_>>()?;

        match name {
            "LENGTH" => {
                let v = vals.into_iter().next().unwrap_or(Value::Null);
                match v {
                    Value::Null => Ok(Value::Null),
                    Value::Text(s) => Ok(Value::Int(s.len() as i64)),
                    _ => Err(MiniDbError::Type("LENGTH requires TEXT".into())),
                }
            }
            "LOWER" => {
                let v = vals.into_iter().next().unwrap_or(Value::Null);
                match v {
                    Value::Null => Ok(Value::Null),
                    Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
                    _ => Err(MiniDbError::Type("LOWER requires TEXT".into())),
                }
            }
            "UPPER" => {
                let v = vals.into_iter().next().unwrap_or(Value::Null);
                match v {
                    Value::Null => Ok(Value::Null),
                    Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
                    _ => Err(MiniDbError::Type("UPPER requires TEXT".into())),
                }
            }
            "SUBSTR" => {
                if vals.len() < 2 {
                    return Err(MiniDbError::Runtime(
                        "SUBSTR requires at least 2 arguments".into(),
                    ));
                }
                match &vals[0] {
                    Value::Null => Ok(Value::Null),
                    Value::Text(s) => {
                        let start = vals[1].as_int()? as usize;
                        let start = if start > 0 { start - 1 } else { 0 };
                        let result = if vals.len() >= 3 {
                            let len = vals[2].as_int()? as usize;
                            s.chars().skip(start).take(len).collect()
                        } else {
                            s.chars().skip(start).collect()
                        };
                        Ok(Value::Text(result))
                    }
                    _ => Err(MiniDbError::Type("SUBSTR requires TEXT".into())),
                }
            }
            "ABS" => {
                let v = vals.into_iter().next().unwrap_or(Value::Null);
                match v {
                    Value::Null => Ok(Value::Null),
                    Value::Int(n) => Ok(Value::Int(n.abs())),
                    Value::Real(f) => Ok(Value::Real(f.abs())),
                    _ => Err(MiniDbError::Type("ABS requires numeric".into())),
                }
            }
            "COALESCE" => {
                for v in vals {
                    if !v.is_null() {
                        return Ok(v);
                    }
                }
                Ok(Value::Null)
            }
            _ => Err(MiniDbError::Runtime(format!("unknown function '{}'", name))),
        }
    }

    /// Evaluate expression as boolean (for WHERE predicates)
    /// Returns true if the expression evaluates to a truthy non-null value
    pub fn eval_bool(expr: &Expr, row: &[Value], schema: &Schema) -> Result<bool> {
        match Self::eval(expr, row, schema)? {
            Value::Bool(b) => Ok(b),
            Value::Int(n) => Ok(n != 0),
            Value::Null => Ok(false),
            other => Err(MiniDbError::Type(format!(
                "expected boolean, got {:?}",
                other
            ))),
        }
    }
}

fn numeric_binop(
    l: &Value,
    r: &Value,
    int_op: fn(i64, i64) -> i64,
    real_op: fn(f64, f64) -> f64,
) -> Result<Value> {
    if l.is_null() || r.is_null() {
        return Ok(Value::Null);
    }
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(int_op(*a, *b))),
        (Value::Real(a), Value::Real(b)) => Ok(Value::Real(real_op(*a, *b))),
        (Value::Int(a), Value::Real(b)) => Ok(Value::Real(real_op(*a as f64, *b))),
        (Value::Real(a), Value::Int(b)) => Ok(Value::Real(real_op(*a, *b as f64))),
        _ => Err(MiniDbError::Type(format!(
            "non-numeric operands: {:?} {:?}",
            l, r
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OutputColumn, Schema};

    fn schema(names: &[&str]) -> Schema {
        Schema::new(
            names
                .iter()
                .map(|n| OutputColumn {
                    name: n.to_string(),
                    table: None,
                    dtype: None,
                })
                .collect(),
        )
    }

    #[test]
    fn test_eval_literal() {
        let s = schema(&[]);
        let expr = Expr::Literal(Value::Int(42));
        assert_eq!(Evaluator::eval(&expr, &[], &s).unwrap(), Value::Int(42));
    }

    #[test]
    fn test_eval_column() {
        let s = schema(&["id", "name"]);
        let row = vec![Value::Int(1), Value::Text("alice".into())];
        let expr = Expr::Column("name".into());
        assert_eq!(
            Evaluator::eval(&expr, &row, &s).unwrap(),
            Value::Text("alice".into())
        );
    }

    #[test]
    fn test_eval_arithmetic() {
        let s = schema(&[]);
        let expr = Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(Expr::Literal(Value::Int(3))),
            right: Box::new(Expr::Literal(Value::Int(4))),
        };
        assert_eq!(Evaluator::eval(&expr, &[], &s).unwrap(), Value::Int(7));
    }

    #[test]
    fn test_eval_null_propagation() {
        let s = schema(&[]);
        let expr = Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(Expr::Literal(Value::Null)),
            right: Box::new(Expr::Literal(Value::Int(1))),
        };
        assert!(Evaluator::eval(&expr, &[], &s).unwrap().is_null());
    }

    #[test]
    fn test_eval_functions() {
        let s = schema(&[]);
        let r = &[];
        // LENGTH
        let e = Expr::Function {
            name: "LENGTH".into(),
            args: vec![Expr::Literal(Value::Text("hello".into()))],
        };
        assert_eq!(Evaluator::eval(&e, r, &s).unwrap(), Value::Int(5));
        // UPPER
        let e = Expr::Function {
            name: "UPPER".into(),
            args: vec![Expr::Literal(Value::Text("hello".into()))],
        };
        assert_eq!(
            Evaluator::eval(&e, r, &s).unwrap(),
            Value::Text("HELLO".into())
        );
        // COALESCE
        let e = Expr::Function {
            name: "COALESCE".into(),
            args: vec![Expr::Literal(Value::Null), Expr::Literal(Value::Int(42))],
        };
        assert_eq!(Evaluator::eval(&e, r, &s).unwrap(), Value::Int(42));
    }

    #[test]
    fn test_three_valued_logic() {
        let s = schema(&[]);
        let r = &[];
        // NULL AND TRUE = NULL
        let e = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::Literal(Value::Null)),
            right: Box::new(Expr::Literal(Value::Bool(true))),
        };
        assert!(Evaluator::eval(&e, r, &s).unwrap().is_null());
        // NULL AND FALSE = FALSE
        let e = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::Literal(Value::Null)),
            right: Box::new(Expr::Literal(Value::Bool(false))),
        };
        assert_eq!(Evaluator::eval(&e, r, &s).unwrap(), Value::Bool(false));
    }
}

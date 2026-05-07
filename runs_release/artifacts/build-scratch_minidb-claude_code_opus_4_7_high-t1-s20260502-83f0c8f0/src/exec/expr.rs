//! Expression evaluation against a schema-bound row.

use crate::error::{MiniDbError, Result};
use crate::parser::ast::{BinaryOp, Expr, UnaryOp};
use crate::types::{DataType, Schema, Tri, Value};

/// Bind a parser AST expression to a specific input schema, resolving column
/// references into ordinal positions. Returns a typed expression tree the
/// evaluator can run row-by-row.
pub fn bind(expr: &Expr, schema: &Schema) -> Result<BoundExpr> {
    match expr {
        Expr::Literal(v) => Ok(BoundExpr::Literal(v.clone())),
        Expr::Column { qualifier, name } => {
            let pos = schema.resolve(qualifier.as_deref(), name)?;
            Ok(BoundExpr::Column(pos))
        }
        Expr::UnaryOp { op, expr } => {
            let inner = bind(expr, schema)?;
            Ok(BoundExpr::Unary(*op, Box::new(inner)))
        }
        Expr::BinaryOp { op, left, right } => Ok(BoundExpr::Binary(
            *op,
            Box::new(bind(left, schema)?),
            Box::new(bind(right, schema)?),
        )),
        Expr::IsNull(e) => Ok(BoundExpr::IsNull(Box::new(bind(e, schema)?))),
        Expr::IsNotNull(e) => Ok(BoundExpr::IsNotNull(Box::new(bind(e, schema)?))),
        Expr::FunctionCall { name, args } => {
            let bargs: Result<Vec<_>> = args.iter().map(|a| bind(a, schema)).collect();
            Ok(BoundExpr::Function(name.clone(), bargs?))
        }
    }
}

#[derive(Debug, Clone)]
pub enum BoundExpr {
    Literal(Value),
    Column(usize),
    Unary(UnaryOp, Box<BoundExpr>),
    Binary(BinaryOp, Box<BoundExpr>, Box<BoundExpr>),
    IsNull(Box<BoundExpr>),
    IsNotNull(Box<BoundExpr>),
    Function(String, Vec<BoundExpr>),
}

pub fn eval(expr: &BoundExpr, row: &[Value]) -> Result<Value> {
    match expr {
        BoundExpr::Literal(v) => Ok(v.clone()),
        BoundExpr::Column(idx) => Ok(row.get(*idx).cloned().unwrap_or(Value::Null)),
        BoundExpr::Unary(op, e) => {
            let v = eval(e, row)?;
            match op {
                UnaryOp::Neg => match v {
                    Value::Null => Ok(Value::Null),
                    Value::Int(n) => Ok(Value::Int(-n)),
                    Value::Real(r) => Ok(Value::Real(-r)),
                    other => Err(MiniDbError::ty(format!(
                        "negation requires a number, got {}",
                        other.type_name()
                    ))),
                },
                UnaryOp::Not => Ok(Tri::not(Tri::from_value(&v)).to_value()),
            }
        }
        BoundExpr::Binary(op, l, r) => {
            let lv = eval(l, row)?;
            let rv = eval(r, row)?;
            apply_binary(*op, &lv, &rv)
        }
        BoundExpr::IsNull(e) => {
            let v = eval(e, row)?;
            Ok(Value::Bool(matches!(v, Value::Null)))
        }
        BoundExpr::IsNotNull(e) => {
            let v = eval(e, row)?;
            Ok(Value::Bool(!matches!(v, Value::Null)))
        }
        BoundExpr::Function(name, args) => {
            let vals: Result<Vec<_>> = args.iter().map(|a| eval(a, row)).collect();
            apply_function(name, &vals?)
        }
    }
}

pub fn eval_predicate(expr: &BoundExpr, row: &[Value]) -> Result<Tri> {
    let v = eval(expr, row)?;
    Ok(Tri::from_value(&v))
}

fn apply_binary(op: BinaryOp, l: &Value, r: &Value) -> Result<Value> {
    use BinaryOp::*;
    if matches!(l, Value::Null) || matches!(r, Value::Null) {
        match op {
            And | Or => {} // handled below with three-valued logic
            _ => return Ok(Value::Null),
        }
    }
    match op {
        Add | Sub | Mul | Div => {
            let (a, b) = Value::promote_numeric(l, r)?;
            match (a, b, op) {
                (Value::Null, _, _) | (_, Value::Null, _) => Ok(Value::Null),
                (Value::Int(x), Value::Int(y), Add) => Ok(Value::Int(x.wrapping_add(y))),
                (Value::Int(x), Value::Int(y), Sub) => Ok(Value::Int(x.wrapping_sub(y))),
                (Value::Int(x), Value::Int(y), Mul) => Ok(Value::Int(x.wrapping_mul(y))),
                (Value::Int(x), Value::Int(y), Div) => {
                    if y == 0 {
                        return Err(MiniDbError::runtime("integer division by zero"));
                    }
                    Ok(Value::Int(x / y))
                }
                (Value::Real(x), Value::Real(y), Add) => Ok(Value::Real(x + y)),
                (Value::Real(x), Value::Real(y), Sub) => Ok(Value::Real(x - y)),
                (Value::Real(x), Value::Real(y), Mul) => Ok(Value::Real(x * y)),
                (Value::Real(x), Value::Real(y), Div) => {
                    if y == 0.0 {
                        return Err(MiniDbError::runtime("real division by zero"));
                    }
                    Ok(Value::Real(x / y))
                }
                _ => Err(MiniDbError::ty("non-numeric arithmetic")),
            }
        }
        Concat => match (
            l.clone().coerce_to(DataType::Text)?,
            r.clone().coerce_to(DataType::Text)?,
        ) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Text(a), Value::Text(b)) => Ok(Value::Text(a + &b)),
            _ => Err(MiniDbError::ty("|| requires text-coercible operands")),
        },
        Eq | NotEq | Lt | LtEq | Gt | GtEq => {
            use std::cmp::Ordering as O;
            let cmp = Value::cmp_sql(l, r);
            let bool_v = match (op, cmp) {
                (_, None) => return Ok(Value::Null),
                (Eq, Some(O::Equal)) => true,
                (NotEq, Some(o)) => o != O::Equal,
                (Eq, Some(_)) => false,
                (Lt, Some(o)) => o == O::Less,
                (LtEq, Some(o)) => o != O::Greater,
                (Gt, Some(o)) => o == O::Greater,
                (GtEq, Some(o)) => o != O::Less,
                _ => unreachable!(),
            };
            Ok(Value::Bool(bool_v))
        }
        And => Ok(Tri::and(Tri::from_value(l), Tri::from_value(r)).to_value()),
        Or => Ok(Tri::or(Tri::from_value(l), Tri::from_value(r)).to_value()),
    }
}

fn apply_function(name: &str, args: &[Value]) -> Result<Value> {
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "LENGTH" => {
            if args.len() != 1 {
                return Err(MiniDbError::ty("LENGTH expects 1 argument"));
            }
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Int(s.chars().count() as i64)),
                other => Err(MiniDbError::ty(format!(
                    "LENGTH requires TEXT, got {}",
                    other.type_name()
                ))),
            }
        }
        "LOWER" => match &args.get(0) {
            None => Err(MiniDbError::ty("LOWER expects 1 argument")),
            Some(Value::Null) => Ok(Value::Null),
            Some(Value::Text(s)) => Ok(Value::Text(s.to_lowercase())),
            Some(other) => Err(MiniDbError::ty(format!(
                "LOWER requires TEXT, got {}",
                other.type_name()
            ))),
        },
        "UPPER" => match &args.get(0) {
            None => Err(MiniDbError::ty("UPPER expects 1 argument")),
            Some(Value::Null) => Ok(Value::Null),
            Some(Value::Text(s)) => Ok(Value::Text(s.to_uppercase())),
            Some(other) => Err(MiniDbError::ty(format!(
                "UPPER requires TEXT, got {}",
                other.type_name()
            ))),
        },
        "ABS" => match args.get(0) {
            None => Err(MiniDbError::ty("ABS expects 1 argument")),
            Some(Value::Null) => Ok(Value::Null),
            Some(Value::Int(n)) => Ok(Value::Int(n.wrapping_abs())),
            Some(Value::Real(r)) => Ok(Value::Real(r.abs())),
            Some(other) => Err(MiniDbError::ty(format!(
                "ABS requires a number, got {}",
                other.type_name()
            ))),
        },
        "SUBSTR" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(MiniDbError::ty("SUBSTR expects 2 or 3 arguments"));
            }
            let s = match &args[0] {
                Value::Null => return Ok(Value::Null),
                Value::Text(s) => s.clone(),
                other => {
                    return Err(MiniDbError::ty(format!(
                        "SUBSTR requires TEXT, got {}",
                        other.type_name()
                    )))
                }
            };
            let start = match &args[1] {
                Value::Null => return Ok(Value::Null),
                Value::Int(n) => *n,
                other => {
                    return Err(MiniDbError::ty(format!(
                        "SUBSTR start must be INT, got {}",
                        other.type_name()
                    )))
                }
            };
            let len = if args.len() == 3 {
                match &args[2] {
                    Value::Null => return Ok(Value::Null),
                    Value::Int(n) => Some(*n),
                    other => {
                        return Err(MiniDbError::ty(format!(
                            "SUBSTR length must be INT, got {}",
                            other.type_name()
                        )))
                    }
                }
            } else {
                None
            };
            Ok(Value::Text(substr(&s, start, len)))
        }
        "COALESCE" => {
            for v in args {
                if !matches!(v, Value::Null) {
                    return Ok(v.clone());
                }
            }
            Ok(Value::Null)
        }
        other => Err(MiniDbError::ty(format!("unknown function: {}", other))),
    }
}

fn substr(s: &str, start: i64, len: Option<i64>) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len() as i64;
    // SQL convention: 1-based start; negative starts measure from the end.
    let start_idx = if start > 0 {
        (start - 1).min(n)
    } else if start == 0 {
        0
    } else {
        let from_end = n + start;
        from_end.max(0)
    };
    let end_idx = match len {
        Some(l) if l < 0 => start_idx,
        Some(l) => (start_idx + l).min(n),
        None => n,
    };
    chars[start_idx as usize..end_idx as usize].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SchemaColumn;

    fn schema_of(cols: &[(&str, &str, DataType)]) -> Schema {
        Schema::new(
            cols.iter()
                .map(|(q, n, t)| SchemaColumn {
                    name: (*n).into(),
                    qualifier: Some((*q).into()),
                    data_type: Some(*t),
                })
                .collect(),
        )
    }

    #[test]
    fn literal_and_column() {
        let s = schema_of(&[("t", "a", DataType::Int)]);
        let e = bind(
            &Expr::BinaryOp {
                op: BinaryOp::Add,
                left: Box::new(Expr::Column {
                    qualifier: Some("t".into()),
                    name: "a".into(),
                }),
                right: Box::new(Expr::Literal(Value::Int(2))),
            },
            &s,
        )
        .unwrap();
        let v = eval(&e, &[Value::Int(3)]).unwrap();
        assert_eq!(v, Value::Int(5));
    }

    #[test]
    fn compare_with_null() {
        let s = schema_of(&[("t", "a", DataType::Int)]);
        let e = bind(
            &Expr::BinaryOp {
                op: BinaryOp::Eq,
                left: Box::new(Expr::Column {
                    qualifier: None,
                    name: "a".into(),
                }),
                right: Box::new(Expr::Literal(Value::Int(1))),
            },
            &s,
        )
        .unwrap();
        let v = eval(&e, &[Value::Null]).unwrap();
        assert_eq!(v, Value::Null);
    }

    #[test]
    fn three_valued_and() {
        let s = schema_of(&[("t", "a", DataType::Bool)]);
        let e = bind(
            &Expr::BinaryOp {
                op: BinaryOp::And,
                left: Box::new(Expr::Column {
                    qualifier: None,
                    name: "a".into(),
                }),
                right: Box::new(Expr::Literal(Value::Null)),
            },
            &s,
        )
        .unwrap();
        let v = eval(&e, &[Value::Bool(false)]).unwrap();
        assert_eq!(v, Value::Bool(false));
    }

    #[test]
    fn function_length() {
        let s = schema_of(&[("t", "n", DataType::Text)]);
        let e = bind(
            &Expr::FunctionCall {
                name: "LENGTH".into(),
                args: vec![Expr::Column {
                    qualifier: None,
                    name: "n".into(),
                }],
            },
            &s,
        )
        .unwrap();
        assert_eq!(
            eval(&e, &[Value::Text("héllo".into())]).unwrap(),
            Value::Int(5)
        );
    }

    #[test]
    fn coalesce() {
        let s = schema_of(&[("t", "a", DataType::Int)]);
        let e = bind(
            &Expr::FunctionCall {
                name: "COALESCE".into(),
                args: vec![
                    Expr::Column {
                        qualifier: None,
                        name: "a".into(),
                    },
                    Expr::Literal(Value::Int(99)),
                ],
            },
            &s,
        )
        .unwrap();
        assert_eq!(eval(&e, &[Value::Null]).unwrap(), Value::Int(99));
        assert_eq!(eval(&e, &[Value::Int(1)]).unwrap(), Value::Int(1));
    }

    #[test]
    fn substr_basic() {
        assert_eq!(substr("hello", 2, Some(3)), "ell");
        assert_eq!(substr("hello", 1, None), "hello");
        assert_eq!(substr("hello", -2, None), "lo");
    }
}

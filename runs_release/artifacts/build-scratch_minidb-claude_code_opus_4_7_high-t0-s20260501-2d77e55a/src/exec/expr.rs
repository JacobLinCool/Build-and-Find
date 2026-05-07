//! Expression evaluation against rows produced by an operator.

use crate::error::{MiniDbError, Result};
use crate::parser::ast::{BinOp, Expr};
use crate::types::{sql_cmp, DataType, TriBool, Value};

use super::{OperatorSchema, Row};

pub fn resolve_column(schema: &OperatorSchema, table: Option<&str>, name: &str) -> Result<usize> {
    let mut found: Option<usize> = None;
    for (i, c) in schema.columns.iter().enumerate() {
        if !c.name.eq_ignore_ascii_case(name) {
            continue;
        }
        if let Some(qual) = table {
            let cq = match &c.table {
                Some(t) => t.to_ascii_lowercase(),
                None => continue,
            };
            if cq != qual.to_ascii_lowercase() {
                continue;
            }
        }
        if found.is_some() {
            return Err(MiniDbError::ty(format!("ambiguous column '{}'", name)));
        }
        found = Some(i);
    }
    found.ok_or_else(|| {
        MiniDbError::ty(match table {
            Some(t) => format!("unknown column '{}.{}'", t, name),
            None => format!("unknown column '{}'", name),
        })
    })
}

pub fn eval(expr: &Expr, schema: &OperatorSchema, row: &Row) -> Result<Value> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::Column { table, name } => {
            let idx = resolve_column(schema, table.as_deref(), name)?;
            Ok(row.get(idx).cloned().unwrap_or(Value::Null))
        }
        Expr::IsNull(inner) => {
            let v = eval(inner, schema, row)?;
            Ok(Value::Bool(v.is_null()))
        }
        Expr::Not(inner) => {
            let v = eval(inner, schema, row)?;
            Ok(TriBool::from_value(&v)?.not().to_value())
        }
        Expr::Neg(inner) => {
            let v = eval(inner, schema, row)?;
            match v {
                Value::Null => Ok(Value::Null),
                Value::Int(i) => Ok(Value::Int(-i)),
                Value::Real(r) => Ok(Value::Real(-r)),
                other => Err(MiniDbError::ty(format!(
                    "cannot negate {}",
                    other.kind_name()
                ))),
            }
        }
        Expr::And(a, b) => {
            let av = TriBool::from_value(&eval(a, schema, row)?)?;
            let bv = TriBool::from_value(&eval(b, schema, row)?)?;
            Ok(av.and(bv).to_value())
        }
        Expr::Or(a, b) => {
            let av = TriBool::from_value(&eval(a, schema, row)?)?;
            let bv = TriBool::from_value(&eval(b, schema, row)?)?;
            Ok(av.or(bv).to_value())
        }
        Expr::Binary { op, left, right } => {
            let l = eval(left, schema, row)?;
            let r = eval(right, schema, row)?;
            apply_binop(*op, l, r)
        }
        Expr::Func { name, args } => {
            let argv: Vec<Value> = args
                .iter()
                .map(|a| eval(a, schema, row))
                .collect::<Result<Vec<_>>>()?;
            apply_function(name, argv)
        }
    }
}

fn apply_binop(op: BinOp, l: Value, r: Value) -> Result<Value> {
    use BinOp::*;
    match op {
        Eq | NotEq | Lt | Le | Gt | Ge => {
            let cmp = sql_cmp(&l, &r)?;
            let v = match cmp {
                None => return Ok(Value::Null),
                Some(o) => match op {
                    Eq => o == std::cmp::Ordering::Equal,
                    NotEq => o != std::cmp::Ordering::Equal,
                    Lt => o == std::cmp::Ordering::Less,
                    Le => o != std::cmp::Ordering::Greater,
                    Gt => o == std::cmp::Ordering::Greater,
                    Ge => o != std::cmp::Ordering::Less,
                    _ => unreachable!(),
                },
            };
            Ok(Value::Bool(v))
        }
        Add | Sub | Mul | Div | Mod => arith(op, l, r),
        Concat => concat(l, r),
    }
}

fn arith(op: BinOp, l: Value, r: Value) -> Result<Value> {
    if l.is_null() || r.is_null() {
        return Ok(Value::Null);
    }
    let promote_to_real = matches!(l, Value::Real(_)) || matches!(r, Value::Real(_));
    if promote_to_real {
        let lf = to_f64(&l)?;
        let rf = to_f64(&r)?;
        let result = match op {
            BinOp::Add => lf + rf,
            BinOp::Sub => lf - rf,
            BinOp::Mul => lf * rf,
            BinOp::Div => {
                if rf == 0.0 {
                    return Err(MiniDbError::runtime("division by zero"));
                }
                lf / rf
            }
            BinOp::Mod => {
                if rf == 0.0 {
                    return Err(MiniDbError::runtime("modulo by zero"));
                }
                lf % rf
            }
            _ => unreachable!(),
        };
        return Ok(Value::Real(result));
    }
    let li = to_i64(&l)?;
    let ri = to_i64(&r)?;
    let v = match op {
        BinOp::Add => li
            .checked_add(ri)
            .ok_or_else(|| MiniDbError::runtime("overflow"))?,
        BinOp::Sub => li
            .checked_sub(ri)
            .ok_or_else(|| MiniDbError::runtime("overflow"))?,
        BinOp::Mul => li
            .checked_mul(ri)
            .ok_or_else(|| MiniDbError::runtime("overflow"))?,
        BinOp::Div => {
            if ri == 0 {
                return Err(MiniDbError::runtime("division by zero"));
            }
            li / ri
        }
        BinOp::Mod => {
            if ri == 0 {
                return Err(MiniDbError::runtime("modulo by zero"));
            }
            li % ri
        }
        _ => unreachable!(),
    };
    Ok(Value::Int(v))
}

fn concat(l: Value, r: Value) -> Result<Value> {
    if l.is_null() || r.is_null() {
        return Ok(Value::Null);
    }
    Ok(Value::Text(format!("{}{}", l, r)))
}

fn to_i64(v: &Value) -> Result<i64> {
    match v {
        Value::Int(i) => Ok(*i),
        other => Err(MiniDbError::ty(format!(
            "expected INT, got {}",
            other.kind_name()
        ))),
    }
}

fn to_f64(v: &Value) -> Result<f64> {
    match v {
        Value::Int(i) => Ok(*i as f64),
        Value::Real(r) => Ok(*r),
        other => Err(MiniDbError::ty(format!(
            "expected numeric, got {}",
            other.kind_name()
        ))),
    }
}

fn apply_function(name: &str, args: Vec<Value>) -> Result<Value> {
    match name {
        "LENGTH" => {
            if args.len() != 1 {
                return Err(MiniDbError::ty("LENGTH expects 1 argument"));
            }
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Int(s.chars().count() as i64)),
                other => Err(MiniDbError::ty(format!(
                    "LENGTH: expected TEXT, got {}",
                    other.kind_name()
                ))),
            }
        }
        "LOWER" => {
            if args.len() != 1 {
                return Err(MiniDbError::ty("LOWER expects 1 argument"));
            }
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
                other => Err(MiniDbError::ty(format!(
                    "LOWER: expected TEXT, got {}",
                    other.kind_name()
                ))),
            }
        }
        "UPPER" => {
            if args.len() != 1 {
                return Err(MiniDbError::ty("UPPER expects 1 argument"));
            }
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
                other => Err(MiniDbError::ty(format!(
                    "UPPER: expected TEXT, got {}",
                    other.kind_name()
                ))),
            }
        }
        "SUBSTR" => {
            if args.len() != 2 && args.len() != 3 {
                return Err(MiniDbError::ty("SUBSTR expects 2 or 3 arguments"));
            }
            if args.iter().any(|v| v.is_null()) {
                return Ok(Value::Null);
            }
            let s = match &args[0] {
                Value::Text(s) => s.clone(),
                other => {
                    return Err(MiniDbError::ty(format!(
                        "SUBSTR: expected TEXT, got {}",
                        other.kind_name()
                    )))
                }
            };
            let start = match &args[1] {
                Value::Int(i) => *i,
                other => {
                    return Err(MiniDbError::ty(format!(
                        "SUBSTR: expected INT start, got {}",
                        other.kind_name()
                    )))
                }
            };
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            // SQL is 1-based; clamp to bounds.
            let s_idx = if start <= 0 { 0 } else { (start - 1).min(len) } as usize;
            let end_idx = if args.len() == 3 {
                let n = match &args[2] {
                    Value::Int(n) => *n,
                    other => {
                        return Err(MiniDbError::ty(format!(
                            "SUBSTR: expected INT length, got {}",
                            other.kind_name()
                        )))
                    }
                };
                if n < 0 {
                    return Err(MiniDbError::runtime("SUBSTR length cannot be negative"));
                }
                ((s_idx as i64) + n).min(len) as usize
            } else {
                len as usize
            };
            Ok(Value::Text(chars[s_idx..end_idx].iter().collect()))
        }
        "ABS" => {
            if args.len() != 1 {
                return Err(MiniDbError::ty("ABS expects 1 argument"));
            }
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Int(i) => Ok(Value::Int(
                    i.checked_abs()
                        .ok_or_else(|| MiniDbError::runtime("ABS overflow"))?,
                )),
                Value::Real(r) => Ok(Value::Real(r.abs())),
                other => Err(MiniDbError::ty(format!(
                    "ABS: expected numeric, got {}",
                    other.kind_name()
                ))),
            }
        }
        "COALESCE" => {
            for a in args {
                if !a.is_null() {
                    return Ok(a);
                }
            }
            Ok(Value::Null)
        }
        other => Err(MiniDbError::ty(format!("unknown function {}", other))),
    }
}

/// Coerce a value into a target column type during INSERT/UPDATE.
pub fn coerce(value: Value, target: DataType) -> Result<Value> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    value.coerce_to(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::ColumnRef;
    use crate::types::Value;

    fn schema() -> OperatorSchema {
        OperatorSchema::new(vec![ColumnRef {
            table: Some("t".into()),
            name: "a".into(),
            dtype: Some(DataType::Int),
        }])
    }

    #[test]
    fn arithmetic_with_nulls() {
        let s = schema();
        let row = vec![Value::Int(5)];
        let e = Expr::Binary {
            op: BinOp::Add,
            left: Box::new(Expr::Column {
                table: None,
                name: "a".into(),
            }),
            right: Box::new(Expr::Literal(Value::Null)),
        };
        let v = eval(&e, &s, &row).unwrap();
        assert!(v.is_null());
    }

    #[test]
    fn three_valued_and() {
        let s = schema();
        let row = vec![Value::Int(5)];
        let e = Expr::And(
            Box::new(Expr::Binary {
                op: BinOp::Eq,
                left: Box::new(Expr::Literal(Value::Null)),
                right: Box::new(Expr::Literal(Value::Null)),
            }),
            Box::new(Expr::Literal(Value::Bool(false))),
        );
        let v = eval(&e, &s, &row).unwrap();
        assert_eq!(v, Value::Bool(false));
    }

    #[test]
    fn substr_one_based() {
        let v = apply_function(
            "SUBSTR",
            vec![Value::Text("hello".into()), Value::Int(2), Value::Int(3)],
        )
        .unwrap();
        assert_eq!(v, Value::Text("ell".into()));
    }

    #[test]
    fn coalesce_returns_first_non_null() {
        let v = apply_function("COALESCE", vec![Value::Null, Value::Null, Value::Int(7)]).unwrap();
        assert_eq!(v, Value::Int(7));
    }

    #[test]
    fn divide_by_zero_errors() {
        let s = schema();
        let row = vec![Value::Int(5)];
        let e = Expr::Binary {
            op: BinOp::Div,
            left: Box::new(Expr::Literal(Value::Int(1))),
            right: Box::new(Expr::Literal(Value::Int(0))),
        };
        assert!(eval(&e, &s, &row).is_err());
    }
}

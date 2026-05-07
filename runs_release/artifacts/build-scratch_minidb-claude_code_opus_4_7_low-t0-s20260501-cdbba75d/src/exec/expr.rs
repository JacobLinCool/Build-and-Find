use super::operator::{ExecSchema, Tuple};
use crate::error::{MiniDbError, Result};
use crate::parser::ast::{BinOp, Expr, UnOp};
use crate::types::Value;

pub fn eval(expr: &Expr, schema: &ExecSchema, row: &Tuple) -> Result<Value> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::Column { table, name } => {
            let idx = schema.resolve(table.as_deref(), name).ok_or_else(|| {
                MiniDbError::Type(format!(
                    "unresolved column {}{}",
                    table
                        .as_deref()
                        .map(|t| format!("{}.", t))
                        .unwrap_or_default(),
                    name
                ))
            })?;
            Ok(row[idx].clone())
        }
        Expr::IsNull(inner) => {
            let v = eval(inner, schema, row)?;
            Ok(Value::Bool(v.is_null()))
        }
        Expr::UnaryOp { op, expr } => {
            let v = eval(expr, schema, row)?;
            match op {
                UnOp::Neg => match v {
                    Value::Null => Ok(Value::Null),
                    Value::Int(i) => Ok(Value::Int(-i)),
                    Value::Real(r) => Ok(Value::Real(-r)),
                    other => Err(MiniDbError::Type(format!("cannot negate {:?}", other))),
                },
                UnOp::Not => match v.truthy() {
                    None => Ok(Value::Null),
                    Some(b) => Ok(Value::Bool(!b)),
                },
            }
        }
        Expr::BinaryOp { op, left, right } => {
            let l = eval(left, schema, row)?;
            let r = eval(right, schema, row)?;
            apply_bin(*op, l, r)
        }
        Expr::FunctionCall { name, args } => {
            let vs: Result<Vec<_>> = args.iter().map(|a| eval(a, schema, row)).collect();
            let vs = vs?;
            apply_func(name, &vs)
        }
    }
}

fn apply_bin(op: BinOp, l: Value, r: Value) -> Result<Value> {
    match op {
        BinOp::And => Ok(three_valued_and(&l, &r)),
        BinOp::Or => Ok(three_valued_or(&l, &r)),
        BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
            let cmp = l.cmp_sql(&r);
            match cmp {
                None => Ok(Value::Null),
                Some(c) => {
                    use std::cmp::Ordering::*;
                    let b = match op {
                        BinOp::Eq => c == Equal,
                        BinOp::NotEq => c != Equal,
                        BinOp::Lt => c == Less,
                        BinOp::LtEq => c != Greater,
                        BinOp::Gt => c == Greater,
                        BinOp::GtEq => c != Less,
                        _ => unreachable!(),
                    };
                    Ok(Value::Bool(b))
                }
            }
        }
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => arith(op, l, r),
        BinOp::Concat => match (l, r) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (a, b) => Ok(Value::Text(format!("{}{}", a, b))),
        },
    }
}

fn three_valued_and(l: &Value, r: &Value) -> Value {
    match (l.truthy(), r.truthy()) {
        (Some(false), _) | (_, Some(false)) => Value::Bool(false),
        (Some(true), Some(true)) => Value::Bool(true),
        _ => Value::Null,
    }
}

fn three_valued_or(l: &Value, r: &Value) -> Value {
    match (l.truthy(), r.truthy()) {
        (Some(true), _) | (_, Some(true)) => Value::Bool(true),
        (Some(false), Some(false)) => Value::Bool(false),
        _ => Value::Null,
    }
}

fn arith(op: BinOp, l: Value, r: Value) -> Result<Value> {
    if l.is_null() || r.is_null() {
        return Ok(Value::Null);
    }
    use Value::*;
    let any_real = matches!(l, Real(_)) || matches!(r, Real(_));
    if any_real {
        let lf = to_f64(&l)?;
        let rf = to_f64(&r)?;
        let v = match op {
            BinOp::Add => lf + rf,
            BinOp::Sub => lf - rf,
            BinOp::Mul => lf * rf,
            BinOp::Div => {
                if rf == 0.0 {
                    return Err(MiniDbError::Runtime("division by zero".into()));
                }
                lf / rf
            }
            _ => unreachable!(),
        };
        Ok(Real(v))
    } else {
        let li = to_i64(&l)?;
        let ri = to_i64(&r)?;
        let v = match op {
            BinOp::Add => li
                .checked_add(ri)
                .ok_or_else(|| MiniDbError::Runtime("int overflow".into()))?,
            BinOp::Sub => li
                .checked_sub(ri)
                .ok_or_else(|| MiniDbError::Runtime("int overflow".into()))?,
            BinOp::Mul => li
                .checked_mul(ri)
                .ok_or_else(|| MiniDbError::Runtime("int overflow".into()))?,
            BinOp::Div => {
                if ri == 0 {
                    return Err(MiniDbError::Runtime("division by zero".into()));
                }
                li / ri
            }
            _ => unreachable!(),
        };
        Ok(Int(v))
    }
}

fn to_f64(v: &Value) -> Result<f64> {
    match v {
        Value::Int(i) => Ok(*i as f64),
        Value::Real(r) => Ok(*r),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        other => Err(MiniDbError::Type(format!("not numeric: {:?}", other))),
    }
}

fn to_i64(v: &Value) -> Result<i64> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Real(r) => Ok(*r as i64),
        Value::Bool(b) => Ok(if *b { 1 } else { 0 }),
        other => Err(MiniDbError::Type(format!("not numeric: {:?}", other))),
    }
}

fn apply_func(name: &str, args: &[Value]) -> Result<Value> {
    match name.to_ascii_uppercase().as_str() {
        "LENGTH" => {
            require_arity("LENGTH", args, 1)?;
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Int(s.chars().count() as i64)),
                other => Err(MiniDbError::Type(format!(
                    "LENGTH expects TEXT, got {:?}",
                    other
                ))),
            }
        }
        "LOWER" => {
            require_arity("LOWER", args, 1)?;
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
                other => Err(MiniDbError::Type(format!(
                    "LOWER expects TEXT, got {:?}",
                    other
                ))),
            }
        }
        "UPPER" => {
            require_arity("UPPER", args, 1)?;
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
                other => Err(MiniDbError::Type(format!(
                    "UPPER expects TEXT, got {:?}",
                    other
                ))),
            }
        }
        "SUBSTR" => {
            // SUBSTR(s, start [, len]) — 1-based start as in SQL
            if args.len() != 2 && args.len() != 3 {
                return Err(MiniDbError::Type("SUBSTR takes 2 or 3 arguments".into()));
            }
            let s = match &args[0] {
                Value::Null => return Ok(Value::Null),
                Value::Text(s) => s.clone(),
                other => return Err(MiniDbError::Type(format!("SUBSTR string arg: {:?}", other))),
            };
            let start = as_int(&args[1])?;
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let zb_start = if start > 0 {
                start - 1
            } else {
                (len + start).max(0)
            };
            let zb_start = zb_start.clamp(0, len) as usize;
            let end = if args.len() == 3 {
                let n = as_int(&args[2])?;
                if n < 0 {
                    return Err(MiniDbError::Runtime(
                        "SUBSTR length cannot be negative".into(),
                    ));
                }
                (zb_start as i64 + n).clamp(0, len) as usize
            } else {
                len as usize
            };
            Ok(Value::Text(chars[zb_start..end].iter().collect()))
        }
        "ABS" => {
            require_arity("ABS", args, 1)?;
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Int(i) => Ok(Value::Int(i.wrapping_abs())),
                Value::Real(r) => Ok(Value::Real(r.abs())),
                other => Err(MiniDbError::Type(format!(
                    "ABS expects numeric, got {:?}",
                    other
                ))),
            }
        }
        "COALESCE" => {
            for a in args {
                if !a.is_null() {
                    return Ok(a.clone());
                }
            }
            Ok(Value::Null)
        }
        other => Err(MiniDbError::Type(format!("unknown function: {}", other))),
    }
}

fn require_arity(name: &str, args: &[Value], n: usize) -> Result<()> {
    if args.len() != n {
        Err(MiniDbError::Type(format!("{} takes {} arg(s)", name, n)))
    } else {
        Ok(())
    }
}

fn as_int(v: &Value) -> Result<i64> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Real(r) => Ok(*r as i64),
        Value::Null => Ok(0),
        other => Err(MiniDbError::Type(format!(
            "expected integer, got {:?}",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::operator::ExecColumn;

    fn schema() -> ExecSchema {
        ExecSchema {
            columns: vec![
                ExecColumn {
                    qualifier: Some("t".into()),
                    name: "a".into(),
                    data_type: None,
                },
                ExecColumn {
                    qualifier: Some("t".into()),
                    name: "b".into(),
                    data_type: None,
                },
            ],
        }
    }

    #[test]
    fn three_valued_and_or() {
        assert!(matches!(
            three_valued_and(&Value::Null, &Value::Bool(false)),
            Value::Bool(false)
        ));
        assert!(matches!(
            three_valued_and(&Value::Null, &Value::Bool(true)),
            Value::Null
        ));
        assert!(matches!(
            three_valued_or(&Value::Null, &Value::Bool(true)),
            Value::Bool(true)
        ));
        assert!(matches!(
            three_valued_or(&Value::Null, &Value::Bool(false)),
            Value::Null
        ));
    }

    #[test]
    fn substr_basic() {
        let s = apply_func(
            "SUBSTR",
            &[Value::Text("hello".into()), Value::Int(2), Value::Int(3)],
        )
        .unwrap();
        assert!(matches!(s, Value::Text(ref t) if t == "ell"));
    }

    #[test]
    fn coalesce_picks_first_non_null() {
        let v = apply_func("COALESCE", &[Value::Null, Value::Null, Value::Int(7)]).unwrap();
        assert!(matches!(v, Value::Int(7)));
    }

    #[test]
    fn arith_division_by_zero() {
        let err = arith(BinOp::Div, Value::Int(1), Value::Int(0)).unwrap_err();
        matches!(err, MiniDbError::Runtime(_));
    }

    #[test]
    fn column_resolution_qualified_and_unqualified() {
        let s = schema();
        let row = vec![Value::Int(1), Value::Int(2)];
        let e = Expr::Column {
            table: Some("t".into()),
            name: "b".into(),
        };
        assert!(matches!(eval(&e, &s, &row).unwrap(), Value::Int(2)));
        let e = Expr::Column {
            table: None,
            name: "a".into(),
        };
        assert!(matches!(eval(&e, &s, &row).unwrap(), Value::Int(1)));
    }
}

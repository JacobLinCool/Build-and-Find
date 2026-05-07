//! Expression evaluation against a single row in some schema.

use crate::error::{MiniDbError, Result};
use crate::parser::ast::{BinaryOp, Expr, UnaryOp};
use crate::plan::physical::Schema;
use crate::types::{compare_values, DataType, Row, TriBool, Value};
use std::cmp::Ordering;

pub fn eval(expr: &Expr, row: &Row, schema: &Schema) -> Result<Value> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::Column { table, name } => {
            let idx = schema.find(table.as_deref(), name).ok_or_else(|| {
                MiniDbError::NoSuchColumn(match table {
                    Some(t) => format!("{}.{}", t, name),
                    None => name.clone(),
                })
            })?;
            Ok(row.0[idx].clone())
        }
        Expr::UnaryOp { op, expr } => {
            let v = eval(expr, row, schema)?;
            match op {
                UnaryOp::Neg => match v {
                    Value::Null => Ok(Value::Null),
                    Value::Int(i) => Ok(Value::Int(-i)),
                    Value::Real(r) => Ok(Value::Real(-r)),
                    other => Err(MiniDbError::Type(format!("cannot negate {:?}", other))),
                },
                UnaryOp::Not => Ok(tribool_to_value(TriBool::from_value(&v).not())),
            }
        }
        Expr::BinaryOp { op, left, right } => {
            let l = eval(left, row, schema)?;
            let r = eval(right, row, schema)?;
            apply_binary(*op, l, r)
        }
        Expr::IsNull(e) => {
            let v = eval(e, row, schema)?;
            Ok(Value::Bool(v.is_null()))
        }
        Expr::IsNotNull(e) => {
            let v = eval(e, row, schema)?;
            Ok(Value::Bool(!v.is_null()))
        }
        Expr::Function { name, args } => {
            let vals: Result<Vec<Value>> = args.iter().map(|a| eval(a, row, schema)).collect();
            let vals = vals?;
            apply_function(name, &vals)
        }
    }
}

fn tribool_to_value(t: TriBool) -> Value {
    match t {
        TriBool::True => Value::Bool(true),
        TriBool::False => Value::Bool(false),
        TriBool::Unknown => Value::Null,
    }
}

pub fn apply_binary(op: BinaryOp, l: Value, r: Value) -> Result<Value> {
    use BinaryOp::*;
    match op {
        And => Ok(tribool_to_value(
            TriBool::from_value(&l).and(TriBool::from_value(&r)),
        )),
        Or => Ok(tribool_to_value(
            TriBool::from_value(&l).or(TriBool::from_value(&r)),
        )),
        Eq | Neq | Lt | Lte | Gt | Gte => {
            if l.is_null() || r.is_null() {
                return Ok(Value::Null);
            }
            let cmp = compare_values(&l, &r)
                .ok_or_else(|| MiniDbError::Type(format!("cannot compare {:?} and {:?}", l, r)))?;
            Ok(Value::Bool(match op {
                Eq => cmp == Ordering::Equal,
                Neq => cmp != Ordering::Equal,
                Lt => cmp == Ordering::Less,
                Lte => cmp != Ordering::Greater,
                Gt => cmp == Ordering::Greater,
                Gte => cmp != Ordering::Less,
                _ => unreachable!(),
            }))
        }
        Concat => {
            if l.is_null() || r.is_null() {
                return Ok(Value::Null);
            }
            Ok(Value::Text(format!("{}{}", l.to_display(), r.to_display())))
        }
        Add | Sub | Mul | Div | Mod => {
            if l.is_null() || r.is_null() {
                return Ok(Value::Null);
            }
            arithmetic(op, l, r)
        }
    }
}

fn arithmetic(op: BinaryOp, l: Value, r: Value) -> Result<Value> {
    use BinaryOp::*;
    let any_real = matches!(l, Value::Real(_)) || matches!(r, Value::Real(_));
    let to_real = |v: &Value| -> Result<f64> {
        match v {
            Value::Int(i) => Ok(*i as f64),
            Value::Real(r) => Ok(*r),
            other => Err(MiniDbError::Type(format!(
                "cannot use {:?} in arithmetic",
                other
            ))),
        }
    };
    let to_int = |v: &Value| -> Result<i64> {
        match v {
            Value::Int(i) => Ok(*i),
            Value::Real(r) => Ok(*r as i64),
            other => Err(MiniDbError::Type(format!(
                "cannot use {:?} in arithmetic",
                other
            ))),
        }
    };
    if any_real {
        let a = to_real(&l)?;
        let b = to_real(&r)?;
        let result = match op {
            Add => a + b,
            Sub => a - b,
            Mul => a * b,
            Div => {
                if b == 0.0 {
                    return Err(MiniDbError::Runtime("division by zero".into()));
                }
                a / b
            }
            Mod => {
                if b == 0.0 {
                    return Err(MiniDbError::Runtime("modulo by zero".into()));
                }
                a % b
            }
            _ => unreachable!(),
        };
        Ok(Value::Real(result))
    } else {
        let a = to_int(&l)?;
        let b = to_int(&r)?;
        let result = match op {
            Add => a
                .checked_add(b)
                .ok_or_else(|| MiniDbError::Runtime("integer overflow".into()))?,
            Sub => a
                .checked_sub(b)
                .ok_or_else(|| MiniDbError::Runtime("integer overflow".into()))?,
            Mul => a
                .checked_mul(b)
                .ok_or_else(|| MiniDbError::Runtime("integer overflow".into()))?,
            Div => {
                if b == 0 {
                    return Err(MiniDbError::Runtime("division by zero".into()));
                }
                a / b
            }
            Mod => {
                if b == 0 {
                    return Err(MiniDbError::Runtime("modulo by zero".into()));
                }
                a % b
            }
            _ => unreachable!(),
        };
        Ok(Value::Int(result))
    }
}

fn apply_function(name: &str, args: &[Value]) -> Result<Value> {
    match name.to_ascii_uppercase().as_str() {
        "LENGTH" => {
            check_arity(name, args, 1)?;
            Ok(match &args[0] {
                Value::Null => Value::Null,
                Value::Text(s) => Value::Int(s.chars().count() as i64),
                other => return Err(MiniDbError::Type(format!("LENGTH on {:?}", other))),
            })
        }
        "LOWER" => {
            check_arity(name, args, 1)?;
            Ok(match &args[0] {
                Value::Null => Value::Null,
                Value::Text(s) => Value::Text(s.to_lowercase()),
                other => return Err(MiniDbError::Type(format!("LOWER on {:?}", other))),
            })
        }
        "UPPER" => {
            check_arity(name, args, 1)?;
            Ok(match &args[0] {
                Value::Null => Value::Null,
                Value::Text(s) => Value::Text(s.to_uppercase()),
                other => return Err(MiniDbError::Type(format!("UPPER on {:?}", other))),
            })
        }
        "ABS" => {
            check_arity(name, args, 1)?;
            Ok(match &args[0] {
                Value::Null => Value::Null,
                Value::Int(i) => Value::Int(i.wrapping_abs()),
                Value::Real(r) => Value::Real(r.abs()),
                other => return Err(MiniDbError::Type(format!("ABS on {:?}", other))),
            })
        }
        "SUBSTR" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(MiniDbError::Type("SUBSTR expects 2 or 3 arguments".into()));
            }
            if matches!(&args[0], Value::Null) {
                return Ok(Value::Null);
            }
            let s = match &args[0] {
                Value::Text(s) => s.clone(),
                _ => return Err(MiniDbError::Type("SUBSTR(text, ...)".into())),
            };
            let start = to_i64(&args[1])?;
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            // SQL SUBSTR is 1-indexed; negative start counts from end.
            let mut start_idx = if start < 0 { len + start } else { start - 1 };
            if start_idx < 0 {
                start_idx = 0;
            }
            let count = if args.len() == 3 {
                to_i64(&args[2])?.max(0)
            } else {
                len - start_idx
            };
            let end_idx = (start_idx + count).min(len);
            if start_idx >= len {
                return Ok(Value::Text(String::new()));
            }
            let out: String = chars[start_idx as usize..end_idx as usize].iter().collect();
            Ok(Value::Text(out))
        }
        "COALESCE" => {
            for a in args {
                if !a.is_null() {
                    return Ok(a.clone());
                }
            }
            Ok(Value::Null)
        }
        other => Err(MiniDbError::Runtime(format!("unknown function {}", other))),
    }
}

fn check_arity(name: &str, args: &[Value], n: usize) -> Result<()> {
    if args.len() != n {
        return Err(MiniDbError::Type(format!(
            "{} expects {} arguments, got {}",
            name,
            n,
            args.len()
        )));
    }
    Ok(())
}

fn to_i64(v: &Value) -> Result<i64> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Real(r) => Ok(*r as i64),
        other => Err(MiniDbError::Type(format!(
            "cannot convert {:?} to integer",
            other
        ))),
    }
}

/// Helper: convert an evaluated value into a TriBool, used for WHERE/CHECK.
pub fn predicate_truth(v: &Value) -> TriBool {
    TriBool::from_value(v)
}

#[allow(dead_code)]
pub fn coerce_for_column(v: Value, dt: DataType) -> Result<Value> {
    v.coerce(dt)
}

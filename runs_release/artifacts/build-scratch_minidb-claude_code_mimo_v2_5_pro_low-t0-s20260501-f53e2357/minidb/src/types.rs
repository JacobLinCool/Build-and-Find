use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColumnType {
    Int,
    Real,
    Text,
    Bool,
}

impl fmt::Display for ColumnType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int => write!(f, "INT"),
            Self::Real => write!(f, "REAL"),
            Self::Text => write!(f, "TEXT"),
            Self::Bool => write!(f, "BOOL"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
    Null,
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Int(_) => "INT",
            Self::Real(_) => "REAL",
            Self::Text(_) => "TEXT",
            Self::Bool(_) => "BOOL",
            Self::Null => "NULL",
        }
    }

    pub fn column_type(&self) -> Option<ColumnType> {
        match self {
            Self::Int(_) => Some(ColumnType::Int),
            Self::Real(_) => Some(ColumnType::Real),
            Self::Text(_) => Some(ColumnType::Text),
            Self::Bool(_) => Some(ColumnType::Bool),
            Self::Null => None,
        }
    }

    pub fn cast_to(&self, ct: ColumnType) -> Result<Value> {
        if self.is_null() {
            return Ok(Value::Null);
        }
        match (self, ct) {
            (Value::Int(_), ColumnType::Int) => Ok(self.clone()),
            (Value::Real(v), ColumnType::Int) => Ok(Value::Int(*v as i64)),
            (Value::Text(s), ColumnType::Int) => {
                s.trim()
                    .parse::<i64>()
                    .map(Value::Int)
                    .map_err(|_| MiniDbError::Type {
                        message: format!("cannot cast '{}' to INT", s),
                    })
            }
            (Value::Bool(b), ColumnType::Int) => Ok(Value::Int(if *b { 1 } else { 0 })),
            (Value::Int(v), ColumnType::Real) => Ok(Value::Real(*v as f64)),
            (Value::Real(_), ColumnType::Real) => Ok(self.clone()),
            (Value::Text(s), ColumnType::Real) => {
                s.trim()
                    .parse::<f64>()
                    .map(Value::Real)
                    .map_err(|_| MiniDbError::Type {
                        message: format!("cannot cast '{}' to REAL", s),
                    })
            }
            (Value::Int(v), ColumnType::Text) => Ok(Value::Text(v.to_string())),
            (Value::Real(v), ColumnType::Text) => Ok(Value::Text(v.to_string())),
            (Value::Text(_), ColumnType::Text) => Ok(self.clone()),
            (Value::Bool(b), ColumnType::Text) => Ok(Value::Text(b.to_string())),
            (Value::Int(v), ColumnType::Bool) => Ok(Value::Bool(*v != 0)),
            (Value::Bool(_), ColumnType::Bool) => Ok(self.clone()),
            (Value::Text(s), ColumnType::Bool) => {
                let lower = s.to_lowercase();
                match lower.as_str() {
                    "true" | "1" | "yes" => Ok(Value::Bool(true)),
                    "false" | "0" | "no" => Ok(Value::Bool(false)),
                    _ => Err(MiniDbError::Type {
                        message: format!("cannot cast '{}' to BOOL", s),
                    }),
                }
            }
            _ => Err(MiniDbError::Type {
                message: format!("cannot cast {} to {}", self.type_name(), ct),
            }),
        }
    }

    pub fn is_compatible_with(&self, ct: ColumnType) -> bool {
        if self.is_null() {
            return true;
        }
        matches!(
            (self, ct),
            (Value::Int(_), ColumnType::Int)
                | (Value::Real(_), ColumnType::Real)
                | (Value::Text(_), ColumnType::Text)
                | (Value::Bool(_), ColumnType::Bool)
                | (Value::Real(_), ColumnType::Int)
                | (Value::Int(_), ColumnType::Real)
        )
    }

    pub fn default_for(ct: ColumnType) -> Value {
        match ct {
            ColumnType::Int => Value::Int(0),
            ColumnType::Real => Value::Real(0.0),
            ColumnType::Text => Value::Text(String::new()),
            ColumnType::Bool => Value::Bool(false),
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.is_null() || other.is_null() {
            return None;
        }
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
            (Value::Real(a), Value::Real(b)) => a.partial_cmp(b),
            (Value::Int(a), Value::Real(b)) => (*a as f64).partial_cmp(b),
            (Value::Real(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
            (Value::Text(a), Value::Text(b)) => a.partial_cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{}", v),
            Self::Real(v) => {
                if v.fract() == 0.0 && v.is_finite() {
                    write!(f, "{}.0", v)
                } else {
                    write!(f, "{}", v)
                }
            }
            Self::Text(v) => write!(f, "{}", v),
            Self::Bool(v) => write!(f, "{}", v),
            Self::Null => write!(f, "NULL"),
        }
    }
}

/// Tri-valued AND: NULL propagates
pub fn tvl_and(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Bool(false), _) | (_, Value::Bool(false)) => Value::Bool(false),
        (Value::Bool(true), Value::Bool(true)) => Value::Bool(true),
        _ => Value::Null,
    }
}

/// Tri-valued OR: NULL propagates
pub fn tvl_or(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Bool(true), _) | (_, Value::Bool(true)) => Value::Bool(true),
        (Value::Bool(false), Value::Bool(false)) => Value::Bool(false),
        _ => Value::Null,
    }
}

/// Tri-valued NOT
pub fn tvl_not(a: &Value) -> Value {
    match a {
        Value::Bool(b) => Value::Bool(!b),
        _ => Value::Null,
    }
}

/// Tri-valued equality
pub fn tvl_eq(a: &Value, b: &Value) -> Value {
    if a.is_null() || b.is_null() {
        return Value::Null;
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Value::Bool(x == y),
        (Value::Real(x), Value::Real(y)) => Value::Bool(x == y),
        (Value::Int(x), Value::Real(y)) => Value::Bool((*x as f64) == *y),
        (Value::Real(x), Value::Int(y)) => Value::Bool(*x == (*y as f64)),
        (Value::Text(x), Value::Text(y)) => Value::Bool(x == y),
        (Value::Bool(x), Value::Bool(y)) => Value::Bool(x == y),
        _ => Value::Bool(false),
    }
}

/// Tri-valued less-than
pub fn tvl_lt(a: &Value, b: &Value) -> Value {
    match a.partial_cmp(b) {
        Some(ord) => Value::Bool(ord == Ordering::Less),
        None => Value::Null,
    }
}

/// Tri-valued less-or-equal
pub fn tvl_le(a: &Value, b: &Value) -> Value {
    match a.partial_cmp(b) {
        Some(ord) => Value::Bool(ord != Ordering::Greater),
        None => Value::Null,
    }
}

/// Tri-valued greater-than
pub fn tvl_gt(a: &Value, b: &Value) -> Value {
    match a.partial_cmp(b) {
        Some(ord) => Value::Bool(ord == Ordering::Greater),
        None => Value::Null,
    }
}

/// Tri-valued greater-or-equal
pub fn tvl_ge(a: &Value, b: &Value) -> Value {
    match a.partial_cmp(b) {
        Some(ord) => Value::Bool(ord != Ordering::Less),
        None => Value::Null,
    }
}

pub fn tvl_neq(a: &Value, b: &Value) -> Value {
    match tvl_eq(a, b) {
        Value::Bool(b) => Value::Bool(!b),
        other => other,
    }
}

/// Arithmetic: +, -, *, /
pub fn tvl_add(a: &Value, b: &Value) -> Result<Value> {
    if a.is_null() || b.is_null() {
        return Ok(Value::Null);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x + y)),
        (Value::Real(x), Value::Real(y)) => Ok(Value::Real(x + y)),
        (Value::Int(x), Value::Real(y)) => Ok(Value::Real(*x as f64 + y)),
        (Value::Real(x), Value::Int(y)) => Ok(Value::Real(x + *y as f64)),
        _ => Err(MiniDbError::Type {
            message: format!("cannot add {} and {}", a.type_name(), b.type_name()),
        }),
    }
}

pub fn tvl_sub(a: &Value, b: &Value) -> Result<Value> {
    if a.is_null() || b.is_null() {
        return Ok(Value::Null);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x - y)),
        (Value::Real(x), Value::Real(y)) => Ok(Value::Real(x - y)),
        (Value::Int(x), Value::Real(y)) => Ok(Value::Real(*x as f64 - y)),
        (Value::Real(x), Value::Int(y)) => Ok(Value::Real(x - *y as f64)),
        _ => Err(MiniDbError::Type {
            message: format!("cannot subtract {} and {}", a.type_name(), b.type_name()),
        }),
    }
}

pub fn tvl_mul(a: &Value, b: &Value) -> Result<Value> {
    if a.is_null() || b.is_null() {
        return Ok(Value::Null);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x * y)),
        (Value::Real(x), Value::Real(y)) => Ok(Value::Real(x * y)),
        (Value::Int(x), Value::Real(y)) => Ok(Value::Real(*x as f64 * y)),
        (Value::Real(x), Value::Int(y)) => Ok(Value::Real(x * *y as f64)),
        _ => Err(MiniDbError::Type {
            message: format!("cannot multiply {} and {}", a.type_name(), b.type_name()),
        }),
    }
}

pub fn tvl_div(a: &Value, b: &Value) -> Result<Value> {
    if a.is_null() || b.is_null() {
        return Ok(Value::Null);
    }
    match (a, b) {
        (Value::Int(_), Value::Int(0)) => Err(MiniDbError::Runtime {
            message: "division by zero".into(),
        }),
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x / y)),
        (Value::Real(x), Value::Real(y)) => Ok(Value::Real(x / y)),
        (Value::Int(x), Value::Real(y)) => Ok(Value::Real(*x as f64 / y)),
        (Value::Real(x), Value::Int(y)) => Ok(Value::Real(x / *y as f64)),
        _ => Err(MiniDbError::Type {
            message: format!("cannot divide {} and {}", a.type_name(), b.type_name()),
        }),
    }
}

/// String concatenation
pub fn tvl_concat(a: &Value, b: &Value) -> Result<Value> {
    if a.is_null() || b.is_null() {
        return Ok(Value::Null);
    }
    match (a, b) {
        (Value::Text(x), Value::Text(y)) => Ok(Value::Text(format!("{}{}", x, y))),
        _ => Err(MiniDbError::Type {
            message: format!("cannot concatenate {} and {}", a.type_name(), b.type_name()),
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub col_type: ColumnType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

impl ColumnDef {
    pub fn new(name: impl Into<String>, col_type: ColumnType) -> Self {
        Self {
            name: name.into(),
            col_type,
            primary_key: false,
            not_null: false,
            unique: false,
            default: None,
        }
    }
}

/// A row is simply a vector of values
pub type Row = Vec<Value>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tvl_and() {
        assert_eq!(
            tvl_and(&Value::Bool(true), &Value::Bool(true)),
            Value::Bool(true)
        );
        assert_eq!(
            tvl_and(&Value::Bool(true), &Value::Bool(false)),
            Value::Bool(false)
        );
        assert_eq!(tvl_and(&Value::Bool(true), &Value::Null), Value::Null);
        assert_eq!(
            tvl_and(&Value::Bool(false), &Value::Null),
            Value::Bool(false)
        );
    }

    #[test]
    fn test_tvl_or() {
        assert_eq!(
            tvl_or(&Value::Bool(false), &Value::Bool(false)),
            Value::Bool(false)
        );
        assert_eq!(tvl_or(&Value::Bool(true), &Value::Null), Value::Bool(true));
        assert_eq!(tvl_or(&Value::Bool(false), &Value::Null), Value::Null);
    }

    #[test]
    fn test_tvl_eq() {
        assert_eq!(tvl_eq(&Value::Int(1), &Value::Int(1)), Value::Bool(true));
        assert_eq!(tvl_eq(&Value::Int(1), &Value::Null), Value::Null);
        assert_eq!(tvl_eq(&Value::Int(1), &Value::Int(2)), Value::Bool(false));
    }

    #[test]
    fn test_arithmetic() {
        assert_eq!(
            tvl_add(&Value::Int(1), &Value::Int(2)).unwrap(),
            Value::Int(3)
        );
        assert_eq!(
            tvl_mul(&Value::Int(3), &Value::Int(4)).unwrap(),
            Value::Int(12)
        );
        assert!(tvl_div(&Value::Int(1), &Value::Int(0)).is_err());
        assert_eq!(tvl_add(&Value::Int(1), &Value::Null).unwrap(), Value::Null);
    }

    #[test]
    fn test_cast() {
        assert_eq!(
            Value::Int(42).cast_to(ColumnType::Real).unwrap(),
            Value::Real(42.0)
        );
        assert_eq!(
            Value::Text("hello".into())
                .cast_to(ColumnType::Text)
                .unwrap(),
            Value::Text("hello".into())
        );
        assert_eq!(
            Value::Int(1).cast_to(ColumnType::Bool).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(Value::Null.cast_to(ColumnType::Int).unwrap(), Value::Null);
    }

    #[test]
    fn test_concat() {
        assert_eq!(
            tvl_concat(&Value::Text("hello".into()), &Value::Text(" world".into())).unwrap(),
            Value::Text("hello world".into())
        );
    }
}

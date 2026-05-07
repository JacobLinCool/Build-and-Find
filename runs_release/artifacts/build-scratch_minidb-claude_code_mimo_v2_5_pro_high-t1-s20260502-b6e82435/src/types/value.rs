use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColumnType {
    Int,
    BigInt,
    Real,
    Text,
    Bool,
}

impl fmt::Display for ColumnType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColumnType::Int => write!(f, "INT"),
            ColumnType::BigInt => write!(f, "BIGINT"),
            ColumnType::Real => write!(f, "REAL"),
            ColumnType::Text => write!(f, "TEXT"),
            ColumnType::Bool => write!(f, "BOOL"),
        }
    }
}

impl ColumnType {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_uppercase().as_str() {
            "INT" | "INTEGER" => Ok(ColumnType::Int),
            "BIGINT" => Ok(ColumnType::BigInt),
            "REAL" | "DOUBLE" | "FLOAT" => Ok(ColumnType::Real),
            "TEXT" | "VARCHAR" | "CHAR" | "STRING" => Ok(ColumnType::Text),
            "BOOL" | "BOOLEAN" => Ok(ColumnType::Bool),
            _ => Err(MiniDbError::Type(format!("unknown type: {}", s))),
        }
    }
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "NULL",
            Value::Int(_) => "INT",
            Value::Real(_) => "REAL",
            Value::Text(_) => "TEXT",
            Value::Bool(_) => "BOOL",
        }
    }

    pub fn coerce_to(&self, col_type: ColumnType) -> Result<Value> {
        if self.is_null() {
            return Ok(Value::Null);
        }
        match (self, col_type) {
            (Value::Int(_), ColumnType::Int | ColumnType::BigInt) => Ok(self.clone()),
            (Value::Real(_), ColumnType::Real) => Ok(self.clone()),
            (Value::Text(_), ColumnType::Text) => Ok(self.clone()),
            (Value::Bool(_), ColumnType::Bool) => Ok(self.clone()),
            (Value::Int(i), ColumnType::Real) => Ok(Value::Real(*i as f64)),
            (Value::Real(r), ColumnType::Int | ColumnType::BigInt) => Ok(Value::Int(*r as i64)),
            (Value::Int(i), ColumnType::Text) => Ok(Value::Text(i.to_string())),
            (Value::Real(r), ColumnType::Text) => Ok(Value::Text(r.to_string())),
            (Value::Bool(b), ColumnType::Text) => Ok(Value::Text(b.to_string())),
            (Value::Text(s), ColumnType::Int | ColumnType::BigInt) => s
                .trim()
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| MiniDbError::Type(format!("cannot convert '{}' to INT", s))),
            (Value::Text(s), ColumnType::Real) => s
                .trim()
                .parse::<f64>()
                .map(Value::Real)
                .map_err(|_| MiniDbError::Type(format!("cannot convert '{}' to REAL", s))),
            (Value::Text(s), ColumnType::Bool) => match s.to_lowercase().as_str() {
                "true" | "1" | "yes" => Ok(Value::Bool(true)),
                "false" | "0" | "no" => Ok(Value::Bool(false)),
                _ => Err(MiniDbError::Type(format!("cannot convert '{}' to BOOL", s))),
            },
            (Value::Int(i), ColumnType::Bool) => Ok(Value::Bool(*i != 0)),
            (Value::Bool(b), ColumnType::Int | ColumnType::BigInt) => Ok(Value::Int(*b as i64)),
            (Value::Bool(b), ColumnType::Real) => Ok(Value::Real(if *b { 1.0 } else { 0.0 })),
            (Value::Real(r), ColumnType::Bool) => Ok(Value::Bool(*r != 0.0)),
            _ => Err(MiniDbError::Type(format!(
                "cannot coerce {} to {}",
                self.type_name(),
                col_type
            ))),
        }
    }

    pub fn default_for(_col_type: ColumnType) -> Value {
        // SQL standard: columns without DEFAULT constraint default to NULL
        Value::Null
    }

    /// SQL three-valued logic AND
    pub fn and(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => {
                if matches!(self, Value::Bool(false)) || matches!(other, Value::Bool(false)) {
                    Value::Bool(false)
                } else {
                    Value::Null
                }
            }
            (Value::Bool(a), Value::Bool(b)) => Value::Bool(*a && *b),
            _ => Value::Null,
        }
    }

    /// SQL three-valued logic OR
    pub fn or(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => {
                if matches!(self, Value::Bool(true)) || matches!(other, Value::Bool(true)) {
                    Value::Bool(true)
                } else {
                    Value::Null
                }
            }
            (Value::Bool(a), Value::Bool(b)) => Value::Bool(*a || *b),
            _ => Value::Null,
        }
    }

    /// SQL three-valued logic NOT
    pub fn not(&self) -> Value {
        match self {
            Value::Null => Value::Null,
            Value::Bool(b) => Value::Bool(!b),
            _ => Value::Null,
        }
    }

    /// SQL IS NULL
    pub fn is_null_check(&self) -> Value {
        Value::Bool(self.is_null())
    }

    /// SQL IS NOT NULL
    pub fn is_not_null_check(&self) -> Value {
        Value::Bool(!self.is_null())
    }

    /// SQL equality with NULL propagation
    pub fn sql_eq(&self, other: &Value) -> Value {
        if self.is_null() || other.is_null() {
            return Value::Null;
        }
        Value::Bool(self == other)
    }

    /// SQL inequality with NULL propagation
    pub fn sql_ne(&self, other: &Value) -> Value {
        if self.is_null() || other.is_null() {
            return Value::Null;
        }
        Value::Bool(self != other)
    }

    /// SQL less than with NULL propagation
    pub fn sql_lt(&self, other: &Value) -> Value {
        match self.partial_cmp(other) {
            Some(ord) => Value::Bool(ord == Ordering::Less),
            None => Value::Null,
        }
    }

    /// SQL less than or equal with NULL propagation
    pub fn sql_le(&self, other: &Value) -> Value {
        match self.partial_cmp(other) {
            Some(ord) => Value::Bool(ord != Ordering::Greater),
            None => Value::Null,
        }
    }

    /// SQL greater than with NULL propagation
    pub fn sql_gt(&self, other: &Value) -> Value {
        match self.partial_cmp(other) {
            Some(ord) => Value::Bool(ord == Ordering::Greater),
            None => Value::Null,
        }
    }

    /// SQL greater than or equal with NULL propagation
    pub fn sql_ge(&self, other: &Value) -> Value {
        match self.partial_cmp(other) {
            Some(ord) => Value::Bool(ord != Ordering::Less),
            None => Value::Null,
        }
    }

    /// SQL addition with NULL propagation
    pub fn sql_add(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a + b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 + b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a + *b as f64)),
            _ => Err(MiniDbError::Type(format!(
                "cannot add {} and {}",
                self.type_name(),
                other.type_name()
            ))),
        }
    }

    /// SQL subtraction with NULL propagation
    pub fn sql_sub(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a - b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 - b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a - *b as f64)),
            _ => Err(MiniDbError::Type(format!(
                "cannot subtract {} from {}",
                other.type_name(),
                self.type_name()
            ))),
        }
    }

    /// SQL multiplication with NULL propagation
    pub fn sql_mul(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a * b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 * b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a * *b as f64)),
            _ => Err(MiniDbError::Type(format!(
                "cannot multiply {} and {}",
                self.type_name(),
                other.type_name()
            ))),
        }
    }

    /// SQL division with NULL propagation
    pub fn sql_div(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(_), Value::Int(0)) => Err(MiniDbError::Runtime("division by zero".into())),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
            (Value::Real(_), Value::Real(0.0)) => {
                Err(MiniDbError::Runtime("division by zero".into()))
            }
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a / b)),
            (Value::Int(a), Value::Real(b)) => {
                if *b == 0.0 {
                    Err(MiniDbError::Runtime("division by zero".into()))
                } else {
                    Ok(Value::Real(*a as f64 / b))
                }
            }
            (Value::Real(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(MiniDbError::Runtime("division by zero".into()))
                } else {
                    Ok(Value::Real(a / *b as f64))
                }
            }
            _ => Err(MiniDbError::Type(format!(
                "cannot divide {} by {}",
                self.type_name(),
                other.type_name()
            ))),
        }
    }

    /// SQL string concatenation
    pub fn sql_concat(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Null, Value::Null) => Value::Null,
            (Value::Null, Value::Text(b)) => Value::Text(b.clone()),
            (Value::Text(a), Value::Null) => Value::Text(a.clone()),
            (Value::Text(a), Value::Text(b)) => {
                let mut s = a.clone();
                s.push_str(b);
                Value::Text(s)
            }
            _ => Value::Null,
        }
    }

    pub fn to_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Real(r) => Some(*r),
            _ => None,
        }
    }

    pub fn to_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => None,
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
            Value::Null => write!(f, "NULL"),
            Value::Int(i) => write!(f, "{}", i),
            Value::Real(r) => {
                if r.fract() == 0.0 && r.is_finite() {
                    write!(f, "{}.0", r)
                } else {
                    write!(f, "{}", r)
                }
            }
            Value::Text(s) => write!(f, "{}", s),
            Value::Bool(b) => write!(f, "{}", b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tri_valued_and() {
        assert_eq!(Value::Bool(true).and(&Value::Bool(true)), Value::Bool(true));
        assert_eq!(
            Value::Bool(true).and(&Value::Bool(false)),
            Value::Bool(false)
        );
        assert_eq!(Value::Bool(true).and(&Value::Null), Value::Null);
        assert_eq!(Value::Bool(false).and(&Value::Null), Value::Bool(false));
    }

    #[test]
    fn test_tri_valued_or() {
        assert_eq!(
            Value::Bool(false).or(&Value::Bool(false)),
            Value::Bool(false)
        );
        assert_eq!(Value::Bool(true).or(&Value::Null), Value::Bool(true));
        assert_eq!(Value::Bool(false).or(&Value::Null), Value::Null);
    }

    #[test]
    fn test_null_propagation() {
        assert_eq!(Value::Int(1).sql_eq(&Value::Null), Value::Null);
        assert_eq!(Value::Null.sql_eq(&Value::Null), Value::Null);
        assert_eq!(Value::Int(1).sql_eq(&Value::Int(1)), Value::Bool(true));
    }

    #[test]
    fn test_arithmetic() {
        assert_eq!(
            Value::Int(1).sql_add(&Value::Int(2)).unwrap(),
            Value::Int(3)
        );
        assert_eq!(Value::Int(1).sql_add(&Value::Null).unwrap(), Value::Null);
    }

    #[test]
    fn test_coerce() {
        assert_eq!(
            Value::Int(42).coerce_to(ColumnType::Real).unwrap(),
            Value::Real(42.0)
        );
        assert_eq!(
            Value::Text("hello".into())
                .coerce_to(ColumnType::Text)
                .unwrap(),
            Value::Text("hello".into())
        );
    }

    #[test]
    fn test_ordering() {
        assert!(Value::Int(1) < Value::Int(2));
        assert!(Value::Real(1.5) < Value::Real(2.5));
        assert_eq!(Value::Int(1).partial_cmp(&Value::Null), None);
    }
}

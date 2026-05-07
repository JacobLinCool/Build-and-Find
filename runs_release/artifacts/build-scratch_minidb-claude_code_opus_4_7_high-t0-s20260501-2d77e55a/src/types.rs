//! Value, column-type, and tri-valued logic primitives.

use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum DataType {
    Int,
    Real,
    Text,
    Bool,
}

impl DataType {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_uppercase().as_str() {
            "INT" | "INTEGER" | "BIGINT" => Ok(Self::Int),
            "REAL" | "DOUBLE" | "FLOAT" => Ok(Self::Real),
            "TEXT" | "VARCHAR" | "STRING" => Ok(Self::Text),
            "BOOL" | "BOOLEAN" => Ok(Self::Bool),
            other => Err(MiniDbError::ty(format!("unknown type {}", other))),
        }
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int => write!(f, "INT"),
            Self::Real => write!(f, "REAL"),
            Self::Text => write!(f, "TEXT"),
            Self::Bool => write!(f, "BOOL"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
}

impl Value {
    pub fn data_type(&self) -> Option<DataType> {
        match self {
            Self::Null => None,
            Self::Int(_) => Some(DataType::Int),
            Self::Real(_) => Some(DataType::Real),
            Self::Text(_) => Some(DataType::Text),
            Self::Bool(_) => Some(DataType::Bool),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Coerce a literal value to a given column type, returning a fresh value.
    /// Errors when types are incompatible.
    pub fn coerce_to(&self, target: DataType) -> Result<Value> {
        match (self, target) {
            (Value::Null, _) => Ok(Value::Null),
            (Value::Int(i), DataType::Int) => Ok(Value::Int(*i)),
            (Value::Int(i), DataType::Real) => Ok(Value::Real(*i as f64)),
            (Value::Real(r), DataType::Real) => Ok(Value::Real(*r)),
            (Value::Real(r), DataType::Int) => {
                if r.fract() == 0.0 && r.is_finite() {
                    Ok(Value::Int(*r as i64))
                } else {
                    Err(MiniDbError::ty(format!("cannot store real {} as INT", r)))
                }
            }
            (Value::Text(s), DataType::Text) => Ok(Value::Text(s.clone())),
            (Value::Bool(b), DataType::Bool) => Ok(Value::Bool(*b)),
            (a, t) => Err(MiniDbError::ty(format!(
                "cannot store {} as {}",
                a.kind_name(),
                t
            ))),
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Null => "NULL",
            Self::Int(_) => "INT",
            Self::Real(_) => "REAL",
            Self::Text(_) => "TEXT",
            Self::Bool(_) => "BOOL",
        }
    }

    /// Total ordering for sort, treating NULL as smallest.
    pub fn cmp_total(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Value::Null, Value::Null) => Ordering::Equal,
            (Value::Null, _) => Ordering::Less,
            (_, Value::Null) => Ordering::Greater,
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Real(a), Value::Real(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Value::Int(a), Value::Real(b)) => {
                (*a as f64).partial_cmp(b).unwrap_or(Ordering::Equal)
            }
            (Value::Real(a), Value::Int(b)) => {
                a.partial_cmp(&(*b as f64)).unwrap_or(Ordering::Equal)
            }
            (Value::Text(a), Value::Text(b)) => a.cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (a, b) => a.kind_name().cmp(b.kind_name()),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::Int(i) => write!(f, "{}", i),
            Self::Real(r) => {
                if r.fract() == 0.0 && r.is_finite() {
                    write!(f, "{:.1}", r)
                } else {
                    write!(f, "{}", r)
                }
            }
            Self::Text(s) => write!(f, "{}", s),
            Self::Bool(b) => write!(f, "{}", if *b { "true" } else { "false" }),
        }
    }
}

/// Three-valued boolean: true/false/unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriBool {
    True,
    False,
    Unknown,
}

impl TriBool {
    pub fn from_bool(b: bool) -> Self {
        if b {
            Self::True
        } else {
            Self::False
        }
    }

    pub fn from_value(v: &Value) -> Result<Self> {
        match v {
            Value::Null => Ok(Self::Unknown),
            Value::Bool(b) => Ok(Self::from_bool(*b)),
            other => Err(MiniDbError::ty(format!(
                "expected boolean, got {}",
                other.kind_name()
            ))),
        }
    }

    pub fn is_true(self) -> bool {
        matches!(self, Self::True)
    }

    pub fn and(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }

    pub fn or(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            _ => Self::Unknown,
        }
    }

    pub fn not(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }

    pub fn to_value(self) -> Value {
        match self {
            Self::True => Value::Bool(true),
            Self::False => Value::Bool(false),
            Self::Unknown => Value::Null,
        }
    }
}

/// Compare two values per SQL semantics; returns Unknown if either is NULL.
pub fn sql_cmp(a: &Value, b: &Value) -> Result<Option<Ordering>> {
    if a.is_null() || b.is_null() {
        return Ok(None);
    }
    Ok(Some(match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Real(x), Value::Real(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Int(x), Value::Real(y)) => (*x as f64).partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Real(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(Ordering::Equal),
        (Value::Text(x), Value::Text(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (x, y) => {
            return Err(MiniDbError::ty(format!(
                "cannot compare {} and {}",
                x.kind_name(),
                y.kind_name()
            )))
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_valued_logic() {
        assert_eq!(TriBool::True.and(TriBool::Unknown), TriBool::Unknown);
        assert_eq!(TriBool::False.and(TriBool::Unknown), TriBool::False);
        assert_eq!(TriBool::True.or(TriBool::Unknown), TriBool::True);
        assert_eq!(TriBool::Unknown.or(TriBool::Unknown), TriBool::Unknown);
        assert_eq!(TriBool::Unknown.not(), TriBool::Unknown);
    }

    #[test]
    fn cmp_null_is_unknown() {
        assert!(sql_cmp(&Value::Null, &Value::Int(1)).unwrap().is_none());
        assert!(sql_cmp(&Value::Int(1), &Value::Int(2)).unwrap().is_some());
    }

    #[test]
    fn coerce_int_to_real() {
        let v = Value::Int(3);
        let coerced = v.coerce_to(DataType::Real).unwrap();
        assert_eq!(coerced, Value::Real(3.0));
    }

    #[test]
    fn coerce_text_to_int_fails() {
        let v = Value::Text("hi".into());
        assert!(v.coerce_to(DataType::Int).is_err());
    }

    #[test]
    fn data_type_parse_aliases() {
        assert_eq!(DataType::parse("BIGINT").unwrap(), DataType::Int);
        assert_eq!(DataType::parse("integer").unwrap(), DataType::Int);
        assert_eq!(DataType::parse("real").unwrap(), DataType::Real);
        assert!(DataType::parse("decimal").is_err());
    }
}

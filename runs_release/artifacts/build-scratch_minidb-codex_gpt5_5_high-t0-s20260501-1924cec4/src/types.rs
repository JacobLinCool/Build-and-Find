use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    Int,
    Real,
    Text,
    Bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    True,
    False,
    Unknown,
}

impl Truth {
    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }

    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            _ => Self::Unknown,
        }
    }

    pub fn negate(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }

    pub fn passes_where(self) -> bool {
        matches!(self, Self::True)
    }
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn cast_to(&self, ty: DataType) -> Result<Value> {
        match (self, ty) {
            (Value::Null, _) => Ok(Value::Null),
            (Value::Int(v), DataType::Int) => Ok(Value::Int(*v)),
            (Value::Int(v), DataType::Real) => Ok(Value::Real(*v as f64)),
            (Value::Real(v), DataType::Real) => Ok(Value::Real(*v)),
            (Value::Real(v), DataType::Int) if v.fract() == 0.0 => Ok(Value::Int(*v as i64)),
            (Value::Text(v), DataType::Text) => Ok(Value::Text(v.clone())),
            (Value::Bool(v), DataType::Bool) => Ok(Value::Bool(*v)),
            _ => Err(MiniDbError::Type(format!(
                "cannot cast {} to {:?}",
                self.type_name(),
                ty
            ))),
        }
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

    pub fn as_truth(&self) -> Truth {
        match self {
            Value::Bool(true) => Truth::True,
            Value::Bool(false) => Truth::False,
            Value::Null => Truth::Unknown,
            _ => Truth::Unknown,
        }
    }

    pub fn sql_cmp(&self, other: &Value) -> Option<Ordering> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => None,
            (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
            (Value::Real(a), Value::Real(b)) => a.partial_cmp(b),
            (Value::Int(a), Value::Real(b)) => (*a as f64).partial_cmp(b),
            (Value::Real(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
            (Value::Text(a), Value::Text(b)) => Some(a.cmp(b)),
            (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }

    pub fn key(&self) -> String {
        match self {
            Value::Null => "0:".to_string(),
            Value::Bool(v) => format!("1:{v}"),
            Value::Int(v) => {
                let shifted = (*v as i128) - (i64::MIN as i128);
                format!("2:{shifted:020}")
            }
            Value::Real(v) => {
                let bits = v.to_bits();
                let sortable = if bits & (1u64 << 63) == 0 {
                    bits | (1u64 << 63)
                } else {
                    !bits
                };
                format!("3:{sortable:020}")
            }
            Value::Text(v) => format!("4:{v}"),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Int(v) => write!(f, "{v}"),
            Value::Real(v) => write!(f, "{v}"),
            Value::Text(v) => write!(f, "{v}"),
            Value::Bool(v) => write!(f, "{v}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub values: Vec<Value>,
}

impl Row {
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }
}

pub fn coerce_numeric(left: &Value, right: &Value) -> Result<(Value, Value)> {
    match (left, right) {
        (Value::Null, _) | (_, Value::Null) => Ok((Value::Null, Value::Null)),
        (Value::Int(_), Value::Int(_)) => Ok((left.clone(), right.clone())),
        (Value::Int(a), Value::Real(b)) => Ok((Value::Real(*a as f64), Value::Real(*b))),
        (Value::Real(a), Value::Int(b)) => Ok((Value::Real(*a), Value::Real(*b as f64))),
        (Value::Real(_), Value::Real(_)) => Ok((left.clone(), right.clone())),
        _ => Err(MiniDbError::Type("expected numeric operands".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truth_tables_preserve_unknown() {
        assert_eq!(Truth::Unknown.and(Truth::True), Truth::Unknown);
        assert_eq!(Truth::Unknown.and(Truth::False), Truth::False);
        assert_eq!(Truth::Unknown.or(Truth::True), Truth::True);
    }

    #[test]
    fn null_comparison_is_unknown() {
        assert_eq!(Value::Null.sql_cmp(&Value::Int(1)), None);
    }
}

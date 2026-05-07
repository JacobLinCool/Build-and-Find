use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

use crate::error::{MiniDbError, Result};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    Int,
    Real,
    Text,
    Bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Truth {
    True,
    False,
    Unknown,
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "NULL",
            Self::Int(_) => "INT",
            Self::Real(_) => "REAL",
            Self::Text(_) => "TEXT",
            Self::Bool(_) => "BOOL",
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn as_truth(&self) -> Result<Truth> {
        match self {
            Value::Bool(true) => Ok(Truth::True),
            Value::Bool(false) => Ok(Truth::False),
            Value::Null => Ok(Truth::Unknown),
            _ => Err(MiniDbError::Type(format!(
                "expected BOOL, got {}",
                self.type_name()
            ))),
        }
    }

    pub fn display(&self) -> String {
        match self {
            Value::Null => "NULL".into(),
            Value::Int(v) => v.to_string(),
            Value::Real(v) => v.to_string(),
            Value::Text(v) => v.clone(),
            Value::Bool(v) => v.to_string(),
        }
    }

    pub fn key(&self) -> String {
        match self {
            Value::Null => "0:".into(),
            Value::Bool(v) => format!("1:{v}"),
            Value::Int(v) => format!("2:{v:020}"),
            Value::Real(v) => format!("3:{:020}", v.to_bits()),
            Value::Text(v) => format!("4:{v}"),
        }
    }
}

pub fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Null, _) | (_, Value::Null) => None,
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Real(x), Value::Real(y)) => x.partial_cmp(y),
        (Value::Int(x), Value::Real(y)) => (*x as f64).partial_cmp(y),
        (Value::Real(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
        (Value::Text(x), Value::Text(y)) => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

impl Truth {
    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (Truth::False, _) | (_, Truth::False) => Truth::False,
            (Truth::True, Truth::True) => Truth::True,
            _ => Truth::Unknown,
        }
    }
    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (Truth::True, _) | (_, Truth::True) => Truth::True,
            (Truth::False, Truth::False) => Truth::False,
            _ => Truth::Unknown,
        }
    }
}

impl std::ops::Not for Truth {
    type Output = Self;
    fn not(self) -> Self::Output {
        match self {
            Truth::True => Truth::False,
            Truth::False => Truth::True,
            Truth::Unknown => Truth::Unknown,
        }
    }
}

pub type Row = Vec<Value>;
pub type RowId = u64;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn null_compare_unknown() {
        assert_eq!(compare(&Value::Null, &Value::Int(1)), None);
        assert_eq!(Truth::Unknown.and(Truth::True), Truth::Unknown);
    }
}

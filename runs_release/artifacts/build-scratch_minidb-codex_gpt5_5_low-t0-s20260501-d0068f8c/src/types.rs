use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, fmt};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub ty: ColumnType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

pub type Row = Vec<Value>;

impl Truth {
    pub fn and(self, rhs: Truth) -> Truth {
        match (self, rhs) {
            (Truth::False, _) | (_, Truth::False) => Truth::False,
            (Truth::True, Truth::True) => Truth::True,
            _ => Truth::Unknown,
        }
    }
    pub fn or(self, rhs: Truth) -> Truth {
        match (self, rhs) {
            (Truth::True, _) | (_, Truth::True) => Truth::True,
            (Truth::False, Truth::False) => Truth::False,
            _ => Truth::Unknown,
        }
    }
    pub fn not(self) -> Truth {
        match self {
            Truth::True => Truth::False,
            Truth::False => Truth::True,
            Truth::Unknown => Truth::Unknown,
        }
    }
    pub fn is_true(self) -> bool {
        matches!(self, Truth::True)
    }
}

impl Value {
    pub fn sql_type(&self) -> &'static str {
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
    pub fn key(&self) -> String {
        match self {
            Value::Null => "0:".into(),
            Value::Bool(v) => format!("1:{v}"),
            Value::Int(v) => format!("2:{v:020}"),
            Value::Real(v) => format!("3:{:020}", (v * 1_000_000.0) as i128),
            Value::Text(v) => format!("4:{v}"),
        }
    }
    pub fn compare(&self, rhs: &Value) -> Option<Ordering> {
        match (self, rhs) {
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
    pub fn cast_for(&self, ty: &ColumnType) -> Result<Value> {
        if matches!(self, Value::Null) {
            return Ok(Value::Null);
        }
        match (self, ty) {
            (Value::Int(v), ColumnType::Int) => Ok(Value::Int(*v)),
            (Value::Int(v), ColumnType::Real) => Ok(Value::Real(*v as f64)),
            (Value::Real(v), ColumnType::Real) => Ok(Value::Real(*v)),
            (Value::Text(v), ColumnType::Text) => Ok(Value::Text(v.clone())),
            (Value::Bool(v), ColumnType::Bool) => Ok(Value::Bool(*v)),
            _ => Err(MiniDbError::Type(format!(
                "cannot store {} as {:?}",
                self.sql_type(),
                ty
            ))),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn three_valued_logic() {
        assert_eq!(Truth::Unknown.and(Truth::True), Truth::Unknown);
        assert_eq!(Truth::Unknown.or(Truth::True), Truth::True);
    }
}

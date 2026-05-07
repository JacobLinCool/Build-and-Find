use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ValueKey {
    Null,
    Bool(bool),
    Int(i64),
    Real(u64),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    Int,
    Real,
    Text,
    Bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub ty: ColumnType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Value,
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

    pub fn negate(self) -> Self {
        match self {
            Truth::True => Truth::False,
            Truth::False => Truth::True,
            Truth::Unknown => Truth::Unknown,
        }
    }

    pub fn accepts(self) -> bool {
        self == Truth::True
    }
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "NULL",
            Value::Int(_) => "INT",
            Value::Real(_) => "REAL",
            Value::Text(_) => "TEXT",
            Value::Bool(_) => "BOOL",
        }
    }

    pub fn key(&self) -> ValueKey {
        match self {
            Value::Null => ValueKey::Null,
            Value::Int(v) => ValueKey::Int(*v),
            Value::Real(v) => {
                let bits = v.to_bits();
                let ordered = if bits & (1_u64 << 63) == 0 {
                    bits ^ (1_u64 << 63)
                } else {
                    !bits
                };
                ValueKey::Real(ordered)
            }
            Value::Text(v) => ValueKey::Text(v.clone()),
            Value::Bool(v) => ValueKey::Bool(*v),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn cast_to(&self, ty: ColumnType) -> Result<Value> {
        match (self, ty) {
            (Value::Null, _) => Ok(Value::Null),
            (Value::Int(v), ColumnType::Int) => Ok(Value::Int(*v)),
            (Value::Int(v), ColumnType::Real) => Ok(Value::Real(*v as f64)),
            (Value::Real(v), ColumnType::Real) => Ok(Value::Real(*v)),
            (Value::Text(v), ColumnType::Text) => Ok(Value::Text(v.clone())),
            (Value::Bool(v), ColumnType::Bool) => Ok(Value::Bool(*v)),
            _ => Err(MiniDbError::Type(format!(
                "cannot store {} in {:?} column",
                self.type_name(),
                ty
            ))),
        }
    }

    pub fn truthy(&self) -> Truth {
        match self {
            Value::Null => Truth::Unknown,
            Value::Bool(true) => Truth::True,
            Value::Bool(false) => Truth::False,
            Value::Int(v) => {
                if *v == 0 {
                    Truth::False
                } else {
                    Truth::True
                }
            }
            Value::Real(v) => {
                if *v == 0.0 {
                    Truth::False
                } else {
                    Truth::True
                }
            }
            Value::Text(v) => {
                if v.is_empty() {
                    Truth::False
                } else {
                    Truth::True
                }
            }
        }
    }

    pub fn compare(&self, other: &Value) -> TruthOrdering {
        if self.is_null() || other.is_null() {
            return TruthOrdering::Unknown;
        }
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => TruthOrdering::Known(a.cmp(b)),
            (Value::Real(a), Value::Real(b)) => match a.partial_cmp(b) {
                Some(order) => TruthOrdering::Known(order),
                None => TruthOrdering::Unknown,
            },
            (Value::Int(a), Value::Real(b)) => match (*a as f64).partial_cmp(b) {
                Some(order) => TruthOrdering::Known(order),
                None => TruthOrdering::Unknown,
            },
            (Value::Real(a), Value::Int(b)) => match a.partial_cmp(&(*b as f64)) {
                Some(order) => TruthOrdering::Known(order),
                None => TruthOrdering::Unknown,
            },
            (Value::Text(a), Value::Text(b)) => TruthOrdering::Known(a.cmp(b)),
            (Value::Bool(a), Value::Bool(b)) => TruthOrdering::Known(a.cmp(b)),
            _ => TruthOrdering::Unknown,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Value::Null => "NULL".to_string(),
            Value::Int(v) => v.to_string(),
            Value::Real(v) => v.to_string(),
            Value::Text(v) => v.clone(),
            Value::Bool(v) => v.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruthOrdering {
    Known(Ordering),
    Unknown,
}

pub type RowValues = Vec<Value>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_valued_logic_matches_sql_shape() {
        assert_eq!(Truth::True.and(Truth::Unknown), Truth::Unknown);
        assert_eq!(Truth::False.and(Truth::Unknown), Truth::False);
        assert_eq!(Truth::Unknown.or(Truth::True), Truth::True);
    }

    #[test]
    fn null_comparison_is_unknown() {
        assert_eq!(Value::Null.compare(&Value::Int(1)), TruthOrdering::Unknown);
    }
}

use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
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

    pub fn column_type(&self) -> Option<ColumnType> {
        match self {
            Self::Null => None,
            Self::Int(_) => Some(ColumnType::Int),
            Self::Real(_) => Some(ColumnType::Real),
            Self::Text(_) => Some(ColumnType::Text),
            Self::Bool(_) => Some(ColumnType::Bool),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn coerce_to(&self, ct: ColumnType) -> Result<Value> {
        if self.is_null() {
            return Ok(Value::Null);
        }
        match (self, ct) {
            (v @ Value::Int(_), ColumnType::Int) => Ok(v.clone()),
            (Value::Int(i), ColumnType::Real) => Ok(Value::Real(*i as f64)),
            (Value::Int(i), ColumnType::Text) => Ok(Value::Text(i.to_string())),
            (Value::Int(0), ColumnType::Bool) => Ok(Value::Bool(false)),
            (Value::Int(_), ColumnType::Bool) => Ok(Value::Bool(true)),
            (v @ Value::Real(_), ColumnType::Real) => Ok(v.clone()),
            (Value::Real(r), ColumnType::Int) => Ok(Value::Int(*r as i64)),
            (Value::Real(r), ColumnType::Text) => Ok(Value::Text(r.to_string())),
            (v @ Value::Text(_), ColumnType::Text) => Ok(v.clone()),
            (Value::Text(s), ColumnType::Int) => {
                let i: i64 = s
                    .trim()
                    .parse()
                    .map_err(|_| MiniDbError::Type(format!("cannot convert '{s}' to INT")))?;
                Ok(Value::Int(i))
            }
            (Value::Text(s), ColumnType::Real) => {
                let r: f64 = s
                    .trim()
                    .parse()
                    .map_err(|_| MiniDbError::Type(format!("cannot convert '{s}' to REAL")))?;
                Ok(Value::Real(r))
            }
            (Value::Text(s), ColumnType::Bool) => match s.to_lowercase().as_str() {
                "true" | "1" | "yes" => Ok(Value::Bool(true)),
                "false" | "0" | "no" => Ok(Value::Bool(false)),
                _ => Err(MiniDbError::Type(format!("cannot convert '{s}' to BOOL"))),
            },
            (v @ Value::Bool(_), ColumnType::Bool) => Ok(v.clone()),
            (Value::Bool(b), ColumnType::Int) => Ok(Value::Int(if *b { 1 } else { 0 })),
            (Value::Bool(b), ColumnType::Text) => Ok(Value::Text(b.to_string())),
            (Value::Bool(_), ColumnType::Real) => {
                Err(MiniDbError::Type("cannot convert BOOL to REAL".to_string()))
            }
            (Value::Real(_), ColumnType::Bool) => {
                Err(MiniDbError::Type("cannot convert REAL to BOOL".to_string()))
            }
            (Value::Text(_), ColumnType::Bool) => {
                Err(MiniDbError::Type("cannot convert TEXT to BOOL".to_string()))
            }
            _ => Err(MiniDbError::Type(format!(
                "cannot convert {} to {}",
                self.type_name(),
                ct
            ))),
        }
    }

    pub fn add(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Value::Null,
            (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_add(*b)),
            (Value::Int(a), Value::Real(b)) => Value::Real(*a as f64 + b),
            (Value::Real(a), Value::Int(b)) => Value::Real(a + *b as f64),
            (Value::Real(a), Value::Real(b)) => Value::Real(a + b),
            _ => Value::Null,
        }
    }

    pub fn sub(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Value::Null,
            (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_sub(*b)),
            (Value::Int(a), Value::Real(b)) => Value::Real(*a as f64 - b),
            (Value::Real(a), Value::Int(b)) => Value::Real(a - *b as f64),
            (Value::Real(a), Value::Real(b)) => Value::Real(a - b),
            _ => Value::Null,
        }
    }

    pub fn mul(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Value::Null,
            (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_mul(*b)),
            (Value::Int(a), Value::Real(b)) => Value::Real(*a as f64 * b),
            (Value::Real(a), Value::Int(b)) => Value::Real(a * *b as f64),
            (Value::Real(a), Value::Real(b)) => Value::Real(a * b),
            _ => Value::Null,
        }
    }

    pub fn div(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Value::Null,
            (_, Value::Int(0)) | (_, Value::Real(0.0)) => Value::Null,
            (Value::Int(a), Value::Int(b)) => Value::Int(a / b),
            (Value::Int(a), Value::Real(b)) => Value::Real(*a as f64 / b),
            (Value::Real(a), Value::Int(b)) => Value::Real(a / *b as f64),
            (Value::Real(a), Value::Real(b)) => Value::Real(a / b),
            _ => Value::Null,
        }
    }

    pub fn concat(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Value::Null,
            (a, b) => Value::Text(format!("{a}{b}")),
        }
    }

    pub fn negate(&self) -> Value {
        match self {
            Value::Null => Value::Null,
            Value::Int(i) => Value::Int(-i),
            Value::Real(r) => Value::Real(-r),
            _ => Value::Null,
        }
    }

    pub fn compare_nulls_last(&self, other: &Value) -> Ordering {
        match (self, other) {
            (Value::Null, Value::Null) => Ordering::Equal,
            (Value::Null, _) => Ordering::Greater,
            (_, Value::Null) => Ordering::Less,
            _ => self.partial_cmp(other).unwrap_or(Ordering::Equal),
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
            Self::Null => write!(f, "NULL"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Real(r) => {
                if r.fract() == 0.0 && r.is_finite() {
                    write!(f, "{r:.1}")
                } else {
                    write!(f, "{r}")
                }
            }
            Self::Text(s) => write!(f, "{s}"),
            Self::Bool(b) => write!(f, "{b}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub col_type: ColumnType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub check_expr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSchema {
    pub name: String,
    pub table: String,
    pub column: String,
}

#[derive(Debug, Clone, Default)]
pub struct Row {
    pub values: Vec<Value>,
}

impl Row {
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_arithmetic() {
        assert_eq!(Value::Int(2).add(&Value::Int(3)), Value::Int(5));
        assert_eq!(Value::Int(5).sub(&Value::Int(3)), Value::Int(2));
        assert_eq!(Value::Int(2).mul(&Value::Int(3)), Value::Int(6));
        assert_eq!(Value::Int(6).div(&Value::Int(3)), Value::Int(2));
        assert!(Value::Int(1).div(&Value::Int(0)).is_null());
    }

    #[test]
    fn test_null_propagation() {
        assert!(Value::Null.add(&Value::Int(1)).is_null());
        assert!(Value::Int(1).add(&Value::Null).is_null());
        assert!(Value::Null.sub(&Value::Int(1)).is_null());
    }

    #[test]
    fn test_coerce() {
        assert_eq!(
            Value::Int(42).coerce_to(ColumnType::Real).unwrap(),
            Value::Real(42.0)
        );
        assert_eq!(
            Value::Int(42).coerce_to(ColumnType::Text).unwrap(),
            Value::Text("42".to_string())
        );
        assert_eq!(
            Value::Text("hello".to_string())
                .coerce_to(ColumnType::Int)
                .unwrap_err()
                .to_string(),
            "Type error: cannot convert 'hello' to INT"
        );
    }

    #[test]
    fn test_compare() {
        assert_eq!(
            Value::Int(1).partial_cmp(&Value::Int(2)),
            Some(Ordering::Less)
        );
        assert_eq!(Value::Null.partial_cmp(&Value::Int(1)), None);
    }

    #[test]
    fn test_concat() {
        assert_eq!(
            Value::Text("hello".to_string()).concat(&Value::Text(" world".to_string())),
            Value::Text("hello world".to_string())
        );
        assert!(Value::Text("a".to_string()).concat(&Value::Null).is_null());
    }

    #[test]
    fn test_negate() {
        assert_eq!(Value::Int(5).negate(), Value::Int(-5));
        assert!(Value::Null.negate().is_null());
    }
}

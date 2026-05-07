use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn sql_type(&self) -> &str {
        match self {
            Value::Null => "NULL",
            Value::Int(_) => "INT",
            Value::Real(_) => "REAL",
            Value::Text(_) => "TEXT",
            Value::Bool(_) => "BOOL",
        }
    }

    pub fn coerce_to_int(&self) -> Result<i64, String> {
        match self {
            Value::Int(v) => Ok(*v),
            Value::Real(v) => Ok(*v as i64),
            Value::Bool(b) => Ok(*b as i64),
            Value::Text(t) => t.parse::<i64>().map_err(|e| e.to_string()),
            Value::Null => Err("NULL cannot be cast to INT".into()),
        }
    }

    pub fn coerce_to_real(&self) -> Result<f64, String> {
        match self {
            Value::Real(v) => Ok(*v),
            Value::Int(v) => Ok(*v as f64),
            Value::Bool(b) => Ok(*b as i64 as f64),
            Value::Text(t) => t.parse::<f64>().map_err(|e| e.to_string()),
            Value::Null => Err("NULL cannot be cast to REAL".into()),
        }
    }

    pub fn coerce_to_text(&self) -> String {
        match self {
            Value::Null => "NULL".to_string(),
            Value::Int(v) => v.to_string(),
            Value::Real(v) => {
                if *v == (*v as i64) as f64 {
                    format!("{}.0", *v as i64)
                } else {
                    v.to_string()
                }
            }
            Value::Text(v) => v.clone(),
            Value::Bool(v) => v.to_string(),
        }
    }

    pub fn coerce_to_bool(&self) -> Result<bool, String> {
        match self {
            Value::Bool(v) => Ok(*v),
            Value::Int(v) => Ok(*v != 0),
            Value::Null => Ok(false),
            _ => Err(format!("Cannot convert {} to BOOL", self.sql_type())),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Int(v) => write!(f, "{}", v),
            Value::Real(v) => {
                if *v == (*v as i64) as f64 {
                    write!(f, "{}.0", *v as i64)
                } else {
                    write!(f, "{}", v)
                }
            }
            Value::Text(v) => write!(f, "{}", v),
            Value::Bool(v) => write!(f, "{}", v),
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    Int,
    BigInt,
    Real,
    Text,
    Bool,
}

impl ColumnType {
    pub fn name(&self) -> &str {
        match self {
            ColumnType::Int => "INT",
            ColumnType::BigInt => "BIGINT",
            ColumnType::Real => "REAL",
            ColumnType::Text => "TEXT",
            ColumnType::Bool => "BOOL",
        }
    }

    pub fn default_value(&self) -> Value {
        match self {
            ColumnType::Int | ColumnType::BigInt => Value::Int(0),
            ColumnType::Real => Value::Real(0.0),
            ColumnType::Text => Value::Text(String::new()),
            ColumnType::Bool => Value::Bool(false),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub col_type: ColumnType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
    pub check: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowId(pub u64);

impl RowId {
    pub fn page_id(&self) -> u64 {
        self.0 / (4096 - 8)
    }

    pub fn slot(&self) -> u16 {
        (self.0 % (4096 - 8)) as u16
    }
}

#[derive(Debug, Clone)]
pub struct Row {
    pub values: Vec<Value>,
}

impl Row {
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_null_propagation() {
        let null = Value::Null;
        let int = Value::Int(5);
        assert_eq!(null.partial_cmp(&int), None);
        assert_eq!(int.partial_cmp(&null), None);
    }

    #[test]
    fn test_value_ordering() {
        let a = Value::Int(1);
        let b = Value::Int(2);
        assert!(a < b);
        assert!(b > a);

        let c = Value::Real(1.5);
        assert!(a < c);
        assert!(c > a);
    }

    #[test]
    fn test_coerce_to_int() {
        assert_eq!(Value::Int(5).coerce_to_int(), Ok(5));
        assert_eq!(Value::Real(3.7).coerce_to_int(), Ok(3));
        assert_eq!(Value::Bool(true).coerce_to_int(), Ok(1));
        assert_eq!(Value::Text("42".into()).coerce_to_int(), Ok(42));
        assert!(Value::Null.coerce_to_int().is_err());
    }

    #[test]
    fn test_coerce_to_real() {
        assert_eq!(Value::Real(3.14).coerce_to_real(), Ok(3.14));
        assert_eq!(Value::Int(5).coerce_to_real(), Ok(5.0));
        assert!(Value::Null.coerce_to_real().is_err());
    }

    #[test]
    fn test_display() {
        assert_eq!(Value::Null.to_string(), "NULL");
        assert_eq!(Value::Int(42).to_string(), "42");
        assert_eq!(Value::Text("hello".into()).to_string(), "hello");
        assert_eq!(Value::Bool(true).to_string(), "true");
    }
}

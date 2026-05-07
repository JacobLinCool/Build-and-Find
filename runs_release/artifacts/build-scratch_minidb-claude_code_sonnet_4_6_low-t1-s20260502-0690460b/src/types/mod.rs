use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    Int,
    BigInt,
    Real,
    Text,
    Bool,
}

impl DataType {
    pub fn name(&self) -> &str {
        match self {
            DataType::Int => "INT",
            DataType::BigInt => "BIGINT",
            DataType::Real => "REAL",
            DataType::Text => "TEXT",
            DataType::Bool => "BOOL",
        }
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// SQL value with tri-valued logic (NULL, TRUE, FALSE for booleans)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => false, // SQL NULL != NULL
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Real(a), Value::Real(b)) => a == b,
            (Value::Int(a), Value::Real(b)) => (*a as f64) == *b,
            (Value::Real(a), Value::Int(b)) => *a == (*b as f64),
            (Value::Text(a), Value::Text(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
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
            // Canonical type ordering for mixed types
            (Value::Int(_) | Value::Real(_), Value::Text(_) | Value::Bool(_)) => Ordering::Less,
            (Value::Text(_) | Value::Bool(_), Value::Int(_) | Value::Real(_)) => Ordering::Greater,
            (Value::Text(_), Value::Bool(_)) => Ordering::Less,
            (Value::Bool(_), Value::Text(_)) => Ordering::Greater,
            _ => Ordering::Equal,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Int(v) => write!(f, "{}", v),
            Value::Real(v) => write!(f, "{}", v),
            Value::Text(v) => write!(f, "{}", v),
            Value::Bool(v) => write!(f, "{}", if *v { "TRUE" } else { "FALSE" }),
        }
    }
}

impl Value {
    pub fn data_type(&self) -> Option<DataType> {
        match self {
            Value::Null => None,
            Value::Int(_) => Some(DataType::Int),
            Value::Real(_) => Some(DataType::Real),
            Value::Text(_) => Some(DataType::Text),
            Value::Bool(_) => Some(DataType::Bool),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn as_int(&self) -> Result<i64> {
        match self {
            Value::Int(v) => Ok(*v),
            Value::Real(v) => Ok(*v as i64),
            Value::Bool(v) => Ok(if *v { 1 } else { 0 }),
            _ => Err(MiniDbError::Type(format!(
                "cannot convert {:?} to INT",
                self
            ))),
        }
    }

    pub fn as_real(&self) -> Result<f64> {
        match self {
            Value::Real(v) => Ok(*v),
            Value::Int(v) => Ok(*v as f64),
            _ => Err(MiniDbError::Type(format!(
                "cannot convert {:?} to REAL",
                self
            ))),
        }
    }

    pub fn as_text(&self) -> Result<&str> {
        match self {
            Value::Text(v) => Ok(v),
            _ => Err(MiniDbError::Type(format!(
                "cannot convert {:?} to TEXT",
                self
            ))),
        }
    }

    pub fn as_bool(&self) -> Result<bool> {
        match self {
            Value::Bool(v) => Ok(*v),
            Value::Int(v) => Ok(*v != 0),
            _ => Err(MiniDbError::Type(format!(
                "cannot convert {:?} to BOOL",
                self
            ))),
        }
    }

    /// Tri-valued SQL equality: returns None when either is NULL
    pub fn sql_eq(&self, other: &Value) -> Option<bool> {
        if self.is_null() || other.is_null() {
            return None;
        }
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(a == b),
            (Value::Real(a), Value::Real(b)) => Some(a == b),
            (Value::Int(a), Value::Real(b)) => Some((*a as f64) == *b),
            (Value::Real(a), Value::Int(b)) => Some(*a == (*b as f64)),
            (Value::Text(a), Value::Text(b)) => Some(a == b),
            (Value::Bool(a), Value::Bool(b)) => Some(a == b),
            _ => Some(false),
        }
    }

    /// Tri-valued SQL comparison: returns None when either is NULL
    pub fn sql_cmp(&self, other: &Value) -> Option<Ordering> {
        if self.is_null() || other.is_null() {
            return None;
        }
        Some(self.cmp(other))
    }

    pub fn coerce_to(&self, dtype: &DataType) -> Result<Value> {
        match (self, dtype) {
            (Value::Null, _) => Ok(Value::Null),
            (Value::Int(v), DataType::Int | DataType::BigInt) => Ok(Value::Int(*v)),
            (Value::Int(v), DataType::Real) => Ok(Value::Real(*v as f64)),
            (Value::Real(v), DataType::Real) => Ok(Value::Real(*v)),
            (Value::Real(v), DataType::Int | DataType::BigInt) => Ok(Value::Int(*v as i64)),
            (Value::Text(v), DataType::Text) => Ok(Value::Text(v.clone())),
            (Value::Bool(v), DataType::Bool) => Ok(Value::Bool(*v)),
            (Value::Bool(v), DataType::Int | DataType::BigInt) => {
                Ok(Value::Int(if *v { 1 } else { 0 }))
            }
            (Value::Int(v), DataType::Bool) => Ok(Value::Bool(*v != 0)),
            _ => Err(MiniDbError::Type(format!(
                "cannot coerce {:?} to {}",
                self, dtype
            ))),
        }
    }

    /// Serialize to bytes for storage
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Value::Null => vec![0],
            Value::Int(v) => {
                let mut b = vec![1];
                b.extend_from_slice(&v.to_le_bytes());
                b
            }
            Value::Real(v) => {
                let mut b = vec![2];
                b.extend_from_slice(&v.to_le_bytes());
                b
            }
            Value::Text(s) => {
                let mut b = vec![3];
                let bytes = s.as_bytes();
                b.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                b.extend_from_slice(bytes);
                b
            }
            Value::Bool(v) => vec![4, if *v { 1 } else { 0 }],
        }
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> Result<(Value, usize)> {
        if data.is_empty() {
            return Err(MiniDbError::Storage("empty value bytes".into()));
        }
        match data[0] {
            0 => Ok((Value::Null, 1)),
            1 => {
                if data.len() < 9 {
                    return Err(MiniDbError::Storage("truncated int".into()));
                }
                let v = i64::from_le_bytes(data[1..9].try_into().unwrap());
                Ok((Value::Int(v), 9))
            }
            2 => {
                if data.len() < 9 {
                    return Err(MiniDbError::Storage("truncated real".into()));
                }
                let v = f64::from_le_bytes(data[1..9].try_into().unwrap());
                Ok((Value::Real(v), 9))
            }
            3 => {
                if data.len() < 5 {
                    return Err(MiniDbError::Storage("truncated text length".into()));
                }
                let len = u32::from_le_bytes(data[1..5].try_into().unwrap()) as usize;
                if data.len() < 5 + len {
                    return Err(MiniDbError::Storage("truncated text data".into()));
                }
                let s = String::from_utf8(data[5..5 + len].to_vec())
                    .map_err(|e| MiniDbError::Storage(e.to_string()))?;
                Ok((Value::Text(s), 5 + len))
            }
            4 => {
                if data.len() < 2 {
                    return Err(MiniDbError::Storage("truncated bool".into()));
                }
                Ok((Value::Bool(data[1] != 0), 2))
            }
            t => Err(MiniDbError::Storage(format!("unknown value tag {}", t))),
        }
    }
}

/// A row is a vector of values
pub type Row = Vec<Value>;

/// Column definition in a schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub dtype: DataType,
    pub not_null: bool,
    pub unique: bool,
    pub primary_key: bool,
    pub default: Option<Value>,
}

impl ColumnDef {
    pub fn new(name: impl Into<String>, dtype: DataType) -> Self {
        ColumnDef {
            name: name.into(),
            dtype,
            not_null: false,
            unique: false,
            primary_key: false,
            default: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_bytes_roundtrip() {
        let values = vec![
            Value::Null,
            Value::Int(42),
            Value::Int(-1),
            Value::Real(3.14),
            Value::Text("hello".into()),
            Value::Bool(true),
            Value::Bool(false),
        ];
        for v in &values {
            let bytes = v.to_bytes();
            let (decoded, _) = Value::from_bytes(&bytes).unwrap();
            match (v, &decoded) {
                (Value::Null, Value::Null) => {}
                (Value::Int(a), Value::Int(b)) => assert_eq!(a, b),
                (Value::Real(a), Value::Real(b)) => assert!((a - b).abs() < 1e-10),
                (Value::Text(a), Value::Text(b)) => assert_eq!(a, b),
                (Value::Bool(a), Value::Bool(b)) => assert_eq!(a, b),
                _ => panic!("type mismatch"),
            }
        }
    }

    #[test]
    fn test_sql_null_semantics() {
        let null = Value::Null;
        let one = Value::Int(1);
        assert_eq!(null.sql_eq(&one), None);
        assert_eq!(null.sql_eq(&null), None);
        assert_eq!(one.sql_eq(&one), Some(true));
    }

    #[test]
    fn test_value_ordering() {
        assert!(Value::Int(1) < Value::Int(2));
        assert!(Value::Null < Value::Int(0));
        assert!(Value::Real(1.5) > Value::Int(1));
    }
}

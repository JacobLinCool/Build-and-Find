use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataType {
    Int,
    BigInt,
    Real,
    Text,
    Bool,
    Null,
}

impl DataType {
    pub fn is_numeric(&self) -> bool {
        matches!(self, DataType::Int | DataType::BigInt | DataType::Real)
    }

    pub fn is_compatible(&self, other: &DataType) -> bool {
        match (self, other) {
            (DataType::Int, DataType::BigInt)
            | (DataType::BigInt, DataType::Int)
            | (DataType::Int, DataType::Real)
            | (DataType::Real, DataType::Int)
            | (DataType::BigInt, DataType::Real)
            | (DataType::Real, DataType::BigInt) => true,
            (a, b) => a == b,
        }
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataType::Int => write!(f, "INT"),
            DataType::BigInt => write!(f, "BIGINT"),
            DataType::Real => write!(f, "REAL"),
            DataType::Text => write!(f, "TEXT"),
            DataType::Bool => write!(f, "BOOL"),
            DataType::Null => write!(f, "NULL"),
        }
    }
}

/// A SQL value supporting three-valued logic (NULL).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
    Null,
}

impl Value {
    pub fn data_type(&self) -> DataType {
        match self {
            Value::Int(_) => DataType::Int,
            Value::Real(_) => DataType::Real,
            Value::Text(_) => DataType::Text,
            Value::Bool(_) => DataType::Bool,
            Value::Null => DataType::Null,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn is_truthy(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Int(i) => Some(*i != 0),
            Value::Null => None,
            _ => Some(true),
        }
    }

    pub fn coerce_to_int(&self) -> Result<i64> {
        match self {
            Value::Int(i) => Ok(*i),
            Value::Real(r) => Ok(*r as i64),
            Value::Bool(b) => Ok(if *b { 1 } else { 0 }),
            Value::Text(s) => s
                .trim()
                .parse::<i64>()
                .map_err(|_| MiniDbError::Type(format!("cannot cast '{}' to INT", s))),
            Value::Null => Err(MiniDbError::Type("cannot cast NULL to INT".into())),
        }
    }

    pub fn coerce_to_real(&self) -> Result<f64> {
        match self {
            Value::Int(i) => Ok(*i as f64),
            Value::Real(r) => Ok(*r),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::Text(s) => s
                .trim()
                .parse::<f64>()
                .map_err(|_| MiniDbError::Type(format!("cannot cast '{}' to REAL", s))),
            Value::Null => Err(MiniDbError::Type("cannot cast NULL to REAL".into())),
        }
    }

    pub fn coerce_to_text(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Real(r) => r.to_string(),
            Value::Text(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Null => "NULL".to_string(),
        }
    }

    /// Three-valued comparison. Returns None if either value is NULL.
    pub fn compare(&self, other: &Value) -> Option<Ordering> {
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

    pub fn add(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.saturating_add(*b))),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a + b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 + b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a + *b as f64)),
            _ => Err(MiniDbError::Type(format!(
                "cannot add {:?} and {:?}",
                self.data_type(),
                other.data_type()
            ))),
        }
    }

    pub fn sub(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.saturating_sub(*b))),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a - b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 - b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a - *b as f64)),
            _ => Err(MiniDbError::Type(format!(
                "cannot subtract {:?} and {:?}",
                self.data_type(),
                other.data_type()
            ))),
        }
    }

    pub fn mul(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.saturating_mul(*b))),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a * b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 * b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a * *b as f64)),
            _ => Err(MiniDbError::Type(format!(
                "cannot multiply {:?} and {:?}",
                self.data_type(),
                other.data_type()
            ))),
        }
    }

    pub fn div(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(_), Value::Int(0)) | (Value::Real(_), Value::Int(0)) => {
                Err(MiniDbError::Runtime("division by zero".into()))
            }
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
            (Value::Real(a), Value::Real(b)) => {
                if *b == 0.0 {
                    Err(MiniDbError::Runtime("division by zero".into()))
                } else {
                    Ok(Value::Real(a / b))
                }
            }
            (Value::Int(a), Value::Real(b)) => {
                if *b == 0.0 {
                    Err(MiniDbError::Runtime("division by zero".into()))
                } else {
                    Ok(Value::Real(*a as f64 / b))
                }
            }
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a / *b as f64)),
            _ => Err(MiniDbError::Type(format!(
                "cannot divide {:?} and {:?}",
                self.data_type(),
                other.data_type()
            ))),
        }
    }

    pub fn concat(&self, other: &Value) -> Value {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Value::Null,
            _ => Value::Text(format!(
                "{}{}",
                self.coerce_to_text(),
                other.coerce_to_text()
            )),
        }
    }

    /// Serialize to bytes for storage
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Value::Null => buf.push(0u8),
            Value::Bool(b) => {
                buf.push(1u8);
                buf.push(if *b { 1 } else { 0 });
            }
            Value::Int(i) => {
                buf.push(2u8);
                buf.extend_from_slice(&i.to_le_bytes());
            }
            Value::Real(r) => {
                buf.push(3u8);
                buf.extend_from_slice(&r.to_le_bytes());
            }
            Value::Text(s) => {
                buf.push(4u8);
                let bytes = s.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(bytes);
            }
        }
        buf
    }

    pub fn from_bytes(data: &[u8]) -> Result<(Value, usize)> {
        if data.is_empty() {
            return Err(MiniDbError::Serialization("empty data".into()));
        }
        match data[0] {
            0 => Ok((Value::Null, 1)),
            1 => {
                if data.len() < 2 {
                    return Err(MiniDbError::Serialization("truncated bool".into()));
                }
                Ok((Value::Bool(data[1] != 0), 2))
            }
            2 => {
                if data.len() < 9 {
                    return Err(MiniDbError::Serialization("truncated int".into()));
                }
                let i = i64::from_le_bytes(data[1..9].try_into().unwrap());
                Ok((Value::Int(i), 9))
            }
            3 => {
                if data.len() < 9 {
                    return Err(MiniDbError::Serialization("truncated real".into()));
                }
                let r = f64::from_le_bytes(data[1..9].try_into().unwrap());
                Ok((Value::Real(r), 9))
            }
            4 => {
                if data.len() < 5 {
                    return Err(MiniDbError::Serialization("truncated text header".into()));
                }
                let len = u32::from_le_bytes(data[1..5].try_into().unwrap()) as usize;
                if data.len() < 5 + len {
                    return Err(MiniDbError::Serialization("truncated text body".into()));
                }
                let s = String::from_utf8(data[5..5 + len].to_vec())
                    .map_err(|_| MiniDbError::Serialization("invalid utf8 in text".into()))?;
                Ok((Value::Text(s), 5 + len))
            }
            t => Err(MiniDbError::Serialization(format!(
                "unknown value tag: {}",
                t
            ))),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{}", i),
            Value::Real(r) => {
                if r.fract() == 0.0 && r.abs() < 1e15 {
                    write!(f, "{:.1}", r)
                } else {
                    write!(f, "{}", r)
                }
            }
            Value::Text(s) => write!(f, "{}", s),
            Value::Bool(b) => write!(f, "{}", if *b { "true" } else { "false" }),
            Value::Null => write!(f, "NULL"),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => false, // SQL NULL != NULL in comparisons
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
        self.compare(other)
    }
}

/// Column definition in a schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

impl Column {
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        Column {
            name: name.into(),
            data_type,
            nullable: true,
            primary_key: false,
            unique: false,
            default: None,
        }
    }
}

/// A row is an ordered list of values
pub type Row = Vec<Value>;

/// Schema: ordered list of columns
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Schema {
    pub columns: Vec<Column>,
}

impl Schema {
    pub fn new(columns: Vec<Column>) -> Self {
        Schema { columns }
    }

    pub fn find_column(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    pub fn serialize_row(&self, row: &Row) -> Result<Vec<u8>> {
        if row.len() != self.columns.len() {
            return Err(MiniDbError::Schema(format!(
                "row has {} columns, schema has {}",
                row.len(),
                self.columns.len()
            )));
        }
        let mut buf = Vec::new();
        for val in row {
            buf.extend_from_slice(&val.to_bytes());
        }
        Ok(buf)
    }

    pub fn deserialize_row(&self, data: &[u8]) -> Result<Row> {
        let mut row = Vec::with_capacity(self.columns.len());
        let mut offset = 0;
        for _ in &self.columns {
            let (val, consumed) = Value::from_bytes(&data[offset..])?;
            row.push(val);
            offset += consumed;
        }
        Ok(row)
    }
}

/// Three-valued logic operations
pub mod tvl {
    use super::Value;

    pub fn and(a: Option<bool>, b: Option<bool>) -> Option<bool> {
        match (a, b) {
            (Some(false), _) | (_, Some(false)) => Some(false),
            (Some(true), Some(true)) => Some(true),
            _ => None,
        }
    }

    pub fn or(a: Option<bool>, b: Option<bool>) -> Option<bool> {
        match (a, b) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(false),
            _ => None,
        }
    }

    pub fn not(a: Option<bool>) -> Option<bool> {
        a.map(|v| !v)
    }

    pub fn eq(a: &Value, b: &Value) -> Option<bool> {
        if a.is_null() || b.is_null() {
            return None;
        }
        Some(a == b)
    }

    pub fn neq(a: &Value, b: &Value) -> Option<bool> {
        eq(a, b).map(|v| !v)
    }

    pub fn lt(a: &Value, b: &Value) -> Option<bool> {
        a.compare(b).map(|o| o == std::cmp::Ordering::Less)
    }

    pub fn le(a: &Value, b: &Value) -> Option<bool> {
        a.compare(b).map(|o| o != std::cmp::Ordering::Greater)
    }

    pub fn gt(a: &Value, b: &Value) -> Option<bool> {
        a.compare(b).map(|o| o == std::cmp::Ordering::Greater)
    }

    pub fn ge(a: &Value, b: &Value) -> Option<bool> {
        a.compare(b).map(|o| o != std::cmp::Ordering::Less)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_serialization() {
        let values = vec![
            Value::Null,
            Value::Bool(true),
            Value::Bool(false),
            Value::Int(42),
            Value::Int(-1000),
            Value::Real(3.14),
            Value::Text("hello world".into()),
        ];

        for v in &values {
            let bytes = v.to_bytes();
            let (decoded, _) = Value::from_bytes(&bytes).unwrap();
            match (v, &decoded) {
                (Value::Null, Value::Null) => {}
                (Value::Bool(a), Value::Bool(b)) => assert_eq!(a, b),
                (Value::Int(a), Value::Int(b)) => assert_eq!(a, b),
                (Value::Real(a), Value::Real(b)) => assert!((a - b).abs() < 1e-10),
                (Value::Text(a), Value::Text(b)) => assert_eq!(a, b),
                _ => panic!("type mismatch"),
            }
        }
    }

    #[test]
    fn test_three_valued_logic() {
        assert_eq!(tvl::and(Some(true), Some(false)), Some(false));
        assert_eq!(tvl::and(Some(false), None), Some(false));
        assert_eq!(tvl::and(None, None), None);
        assert_eq!(tvl::or(Some(true), None), Some(true));
        assert_eq!(tvl::or(Some(false), None), None);
        assert_eq!(tvl::not(None), None);
        assert_eq!(tvl::not(Some(true)), Some(false));
    }

    #[test]
    fn test_arithmetic() {
        let a = Value::Int(10);
        let b = Value::Int(3);
        assert_eq!(a.add(&b).unwrap(), Value::Int(13));
        assert_eq!(a.sub(&b).unwrap(), Value::Int(7));
        assert_eq!(a.mul(&b).unwrap(), Value::Int(30));
        assert_eq!(a.div(&b).unwrap(), Value::Int(3));

        let null = Value::Null;
        assert!(matches!(null.add(&a).unwrap(), Value::Null));
    }

    #[test]
    fn test_schema_row_roundtrip() {
        let schema = Schema::new(vec![
            Column::new("id", DataType::Int),
            Column::new("name", DataType::Text),
            Column::new("score", DataType::Real),
        ]);
        let row = vec![
            Value::Int(1),
            Value::Text("Alice".into()),
            Value::Real(99.5),
        ];
        let bytes = schema.serialize_row(&row).unwrap();
        let decoded = schema.deserialize_row(&bytes).unwrap();
        assert_eq!(decoded[0], Value::Int(1));
        assert_eq!(decoded[1], Value::Text("Alice".into()));
    }
}

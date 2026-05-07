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
}

impl DataType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "INT" | "INTEGER" => Some(DataType::Int),
            "BIGINT" => Some(DataType::BigInt),
            "REAL" | "FLOAT" | "DOUBLE" => Some(DataType::Real),
            "TEXT" | "VARCHAR" | "CHAR" | "STRING" => Some(DataType::Text),
            "BOOL" | "BOOLEAN" => Some(DataType::Bool),
            _ => None,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, DataType::Int | DataType::BigInt | DataType::Real)
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
    Null,
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Int(a), Value::Real(b)) => (*a as f64) == *b,
            (Value::Real(a), Value::Int(b)) => *a == (*b as f64),
            (Value::Real(a), Value::Real(b)) => a == b,
            (Value::Text(a), Value::Text(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Null, Value::Null) => false, // NULL != NULL in SQL
            _ => false,
        }
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.sql_compare(other)?)
    }
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// SQL three-valued comparison. Returns None if either is NULL.
    pub fn sql_compare(&self, other: &Value) -> Option<Ordering> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => None,
            (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
            (Value::Int(a), Value::Real(b)) => (*a as f64).partial_cmp(b),
            (Value::Real(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
            (Value::Real(a), Value::Real(b)) => a.partial_cmp(b),
            (Value::Text(a), Value::Text(b)) => Some(a.cmp(b)),
            (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }

    /// Returns Some(true/false) or None for NULL
    pub fn sql_eq(&self, other: &Value) -> Option<bool> {
        self.sql_compare(other).map(|o| o == Ordering::Equal)
    }

    pub fn sql_lt(&self, other: &Value) -> Option<bool> {
        self.sql_compare(other).map(|o| o == Ordering::Less)
    }

    pub fn sql_le(&self, other: &Value) -> Option<bool> {
        self.sql_compare(other).map(|o| o != Ordering::Greater)
    }

    pub fn sql_gt(&self, other: &Value) -> Option<bool> {
        self.sql_compare(other).map(|o| o == Ordering::Greater)
    }

    pub fn sql_ge(&self, other: &Value) -> Option<bool> {
        self.sql_compare(other).map(|o| o != Ordering::Less)
    }

    pub fn data_type(&self) -> Option<DataType> {
        match self {
            Value::Int(_) => Some(DataType::Int),
            Value::Real(_) => Some(DataType::Real),
            Value::Text(_) => Some(DataType::Text),
            Value::Bool(_) => Some(DataType::Bool),
            Value::Null => None,
        }
    }

    pub fn coerce_to(&self, dt: &DataType) -> Result<Value> {
        match (self, dt) {
            (Value::Null, _) => Ok(Value::Null),
            (Value::Int(i), DataType::Int) => Ok(Value::Int(*i)),
            (Value::Int(i), DataType::BigInt) => Ok(Value::Int(*i)),
            (Value::Int(i), DataType::Real) => Ok(Value::Real(*i as f64)),
            (Value::Int(i), DataType::Bool) => Ok(Value::Bool(*i != 0)),
            (Value::Int(i), DataType::Text) => Ok(Value::Text(i.to_string())),
            (Value::Real(r), DataType::Real) => Ok(Value::Real(*r)),
            (Value::Real(r), DataType::Int) => Ok(Value::Int(*r as i64)),
            (Value::Real(r), DataType::BigInt) => Ok(Value::Int(*r as i64)),
            (Value::Real(r), DataType::Text) => Ok(Value::Text(r.to_string())),
            (Value::Text(s), DataType::Text) => Ok(Value::Text(s.clone())),
            (Value::Text(s), DataType::Int) => s
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| MiniDbError::TypeMismatch(format!("cannot coerce '{}' to INT", s))),
            (Value::Text(s), DataType::Real) => s
                .parse::<f64>()
                .map(Value::Real)
                .map_err(|_| MiniDbError::TypeMismatch(format!("cannot coerce '{}' to REAL", s))),
            (Value::Bool(b), DataType::Bool) => Ok(Value::Bool(*b)),
            (Value::Bool(b), DataType::Int) => Ok(Value::Int(if *b { 1 } else { 0 })),
            (Value::Bool(b), DataType::Text) => Ok(Value::Text(b.to_string())),
            _ => Err(MiniDbError::TypeMismatch(format!(
                "cannot coerce {:?} to {:?}",
                self, dt
            ))),
        }
    }

    pub fn add(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_add(*b))),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 + b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a + *b as f64)),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a + b)),
            _ => Err(MiniDbError::TypeMismatch(format!(
                "cannot add {:?} and {:?}",
                self, other
            ))),
        }
    }

    pub fn sub(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_sub(*b))),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 - b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a - *b as f64)),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a - b)),
            _ => Err(MiniDbError::TypeMismatch(format!(
                "cannot subtract {:?} and {:?}",
                self, other
            ))),
        }
    }

    pub fn mul(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_mul(*b))),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 * b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a * *b as f64)),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a * b)),
            _ => Err(MiniDbError::TypeMismatch(format!(
                "cannot multiply {:?} and {:?}",
                self, other
            ))),
        }
    }

    pub fn div(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(MiniDbError::DivisionByZero)
                } else {
                    Ok(Value::Int(a / b))
                }
            }
            (Value::Int(a), Value::Real(b)) => {
                if *b == 0.0 {
                    Err(MiniDbError::DivisionByZero)
                } else {
                    Ok(Value::Real(*a as f64 / b))
                }
            }
            (Value::Real(a), Value::Int(b)) => {
                if *b == 0 {
                    Err(MiniDbError::DivisionByZero)
                } else {
                    Ok(Value::Real(a / *b as f64))
                }
            }
            (Value::Real(a), Value::Real(b)) => {
                if *b == 0.0 {
                    Err(MiniDbError::DivisionByZero)
                } else {
                    Ok(Value::Real(a / b))
                }
            }
            _ => Err(MiniDbError::TypeMismatch(format!(
                "cannot divide {:?} by {:?}",
                self, other
            ))),
        }
    }

    pub fn concat(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Text(a), Value::Text(b)) => Ok(Value::Text(format!("{}{}", a, b))),
            (a, b) => Ok(Value::Text(format!("{}{}", a, b))),
        }
    }

    pub fn negate(&self) -> Result<Value> {
        match self {
            Value::Null => Ok(Value::Null),
            Value::Int(i) => Ok(Value::Int(-i)),
            Value::Real(r) => Ok(Value::Real(-r)),
            _ => Err(MiniDbError::TypeMismatch(format!(
                "cannot negate {:?}",
                self
            ))),
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Int(i) => Some(*i != 0),
            Value::Null => None,
            _ => None,
        }
    }

    /// Encode value as bytes for BTree key (comparable ordering).
    pub fn encode_key(&self) -> Vec<u8> {
        match self {
            Value::Null => vec![0u8],
            Value::Bool(b) => vec![1u8, if *b { 1 } else { 0 }],
            Value::Int(i) => {
                let mut v = vec![2u8];
                // Flip sign bit for proper ordering
                let u = (i ^ i64::MIN) as u64;
                v.extend_from_slice(&u.to_be_bytes());
                v
            }
            Value::Real(r) => {
                let mut v = vec![3u8];
                let bits = r.to_bits();
                // IEEE 754 ordering trick
                let bits = if *r < 0.0 {
                    bits ^ u64::MAX
                } else {
                    bits ^ (1u64 << 63)
                };
                v.extend_from_slice(&bits.to_be_bytes());
                v
            }
            Value::Text(s) => {
                let mut v = vec![4u8];
                v.extend_from_slice(s.as_bytes());
                v
            }
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

pub type Row = Vec<Value>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub not_null: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

impl ColumnDef {
    pub fn new(name: &str, data_type: DataType) -> Self {
        ColumnDef {
            name: name.to_string(),
            data_type,
            not_null: false,
            primary_key: false,
            unique: false,
            default: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub columns: Vec<(String, DataType)>,
}

impl Schema {
    pub fn new(columns: Vec<(String, DataType)>) -> Self {
        Schema { columns }
    }

    pub fn empty() -> Self {
        Schema { columns: vec![] }
    }

    pub fn col_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|(n, _)| n == name)
    }

    pub fn col_index_icase(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(name))
    }

    pub fn append(&self, other: &Schema, prefix: Option<&str>) -> Schema {
        let mut cols = self.columns.clone();
        for (n, t) in &other.columns {
            let name = if let Some(p) = prefix {
                format!("{}.{}", p, n)
            } else {
                n.clone()
            };
            cols.push((name, t.clone()));
        }
        Schema { columns: cols }
    }
}

/// Serialize a row to bytes for heap storage.
pub fn serialize_row(row: &Row) -> Vec<u8> {
    let mut buf = Vec::new();
    let ncols = row.len() as u16;
    buf.extend_from_slice(&ncols.to_le_bytes());

    // null bitmap
    let bitmap_len = (row.len() + 7) / 8;
    let mut bitmap = vec![0u8; bitmap_len];
    for (i, v) in row.iter().enumerate() {
        if v.is_null() {
            bitmap[i / 8] |= 1 << (i % 8);
        }
    }
    buf.extend_from_slice(&bitmap);

    for v in row {
        match v {
            Value::Null => {}
            Value::Int(i) => buf.extend_from_slice(&i.to_le_bytes()),
            Value::Real(r) => buf.extend_from_slice(&r.to_bits().to_le_bytes()),
            Value::Bool(b) => buf.push(if *b { 1 } else { 0 }),
            Value::Text(s) => {
                let bytes = s.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(bytes);
            }
        }
    }
    buf
}

/// Deserialize a row from bytes given the table schema column types.
pub fn deserialize_row(data: &[u8], types: &[DataType]) -> Result<Row> {
    if data.len() < 2 {
        return Err(MiniDbError::StorageCorruption(
            "row data too short".to_string(),
        ));
    }
    let ncols = u16::from_le_bytes([data[0], data[1]]) as usize;
    if ncols != types.len() {
        return Err(MiniDbError::StorageCorruption(format!(
            "row has {} cols, schema has {}",
            ncols,
            types.len()
        )));
    }
    let bitmap_len = (ncols + 7) / 8;
    if data.len() < 2 + bitmap_len {
        return Err(MiniDbError::StorageCorruption("row data too short".into()));
    }
    let bitmap = &data[2..2 + bitmap_len];
    let mut pos = 2 + bitmap_len;

    let mut row = Vec::with_capacity(ncols);
    for (i, dt) in types.iter().enumerate() {
        let is_null = (bitmap[i / 8] >> (i % 8)) & 1 == 1;
        if is_null {
            row.push(Value::Null);
            continue;
        }
        match dt {
            DataType::Int | DataType::BigInt => {
                if pos + 8 > data.len() {
                    return Err(MiniDbError::StorageCorruption("truncated int".into()));
                }
                let v = i64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
                row.push(Value::Int(v));
                pos += 8;
            }
            DataType::Real => {
                if pos + 8 > data.len() {
                    return Err(MiniDbError::StorageCorruption("truncated real".into()));
                }
                let bits = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
                row.push(Value::Real(f64::from_bits(bits)));
                pos += 8;
            }
            DataType::Bool => {
                if pos >= data.len() {
                    return Err(MiniDbError::StorageCorruption("truncated bool".into()));
                }
                row.push(Value::Bool(data[pos] != 0));
                pos += 1;
            }
            DataType::Text => {
                if pos + 4 > data.len() {
                    return Err(MiniDbError::StorageCorruption("truncated text len".into()));
                }
                let len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(MiniDbError::StorageCorruption("truncated text".into()));
                }
                let s = std::str::from_utf8(&data[pos..pos + len])
                    .map_err(|_| MiniDbError::StorageCorruption("invalid utf8".into()))?;
                row.push(Value::Text(s.to_string()));
                pos += len;
            }
        }
    }
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_compare() {
        assert_eq!(
            Value::Int(1).sql_compare(&Value::Int(2)),
            Some(Ordering::Less)
        );
        assert_eq!(Value::Null.sql_compare(&Value::Int(1)), None);
        assert_eq!(
            Value::Text("a".into()).sql_compare(&Value::Text("b".into())),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn test_arithmetic() {
        assert_eq!(Value::Int(5).add(&Value::Int(3)).unwrap(), Value::Int(8));
        assert_eq!(Value::Int(5).div(&Value::Int(2)).unwrap(), Value::Int(2));
        assert!(Value::Int(1).div(&Value::Int(0)).is_err());
        assert!(Value::Int(1).add(&Value::Null).unwrap().is_null());
    }

    #[test]
    fn test_serialize_deserialize() {
        let row: Row = vec![
            Value::Int(42),
            Value::Text("hello".into()),
            Value::Null,
            Value::Bool(true),
            Value::Real(3.14),
        ];
        let types = vec![
            DataType::Int,
            DataType::Text,
            DataType::Text,
            DataType::Bool,
            DataType::Real,
        ];
        let bytes = serialize_row(&row);
        let restored = deserialize_row(&bytes, &types).unwrap();
        assert_eq!(restored[0], Value::Int(42));
        assert_eq!(restored[1], Value::Text("hello".into()));
        assert!(restored[2].is_null());
        assert_eq!(restored[3], Value::Bool(true));
        if let Value::Real(r) = restored[4] {
            assert!((r - 3.14).abs() < 1e-10);
        } else {
            panic!("expected Real");
        }
    }

    #[test]
    fn test_encode_key_ordering() {
        let mut keys: Vec<Value> = vec![
            Value::Int(100),
            Value::Int(-5),
            Value::Int(0),
            Value::Int(50),
        ];
        keys.sort_by(|a, b| a.encode_key().cmp(&b.encode_key()));
        assert_eq!(keys[0], Value::Int(-5));
        assert_eq!(keys[1], Value::Int(0));
        assert_eq!(keys[2], Value::Int(50));
        assert_eq!(keys[3], Value::Int(100));
    }
}

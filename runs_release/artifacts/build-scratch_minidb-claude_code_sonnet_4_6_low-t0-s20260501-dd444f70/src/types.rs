use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataType {
    Int,
    BigInt,
    Real,
    Text,
    Bool,
}

impl DataType {
    pub fn from_str(s: &str) -> Option<DataType> {
        match s.to_uppercase().as_str() {
            "INT" | "INTEGER" => Some(DataType::Int),
            "BIGINT" => Some(DataType::BigInt),
            "REAL" | "FLOAT" | "DOUBLE" => Some(DataType::Real),
            "TEXT" | "VARCHAR" | "CHAR" | "STRING" => Some(DataType::Text),
            "BOOL" | "BOOLEAN" => Some(DataType::Bool),
            _ => None,
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

impl ColumnDef {
    pub fn new(name: &str, data_type: DataType) -> Self {
        ColumnDef {
            name: name.to_string(),
            data_type,
            nullable: true,
            primary_key: false,
            unique: false,
            default: None,
        }
    }
}

pub type Row = Vec<Value>;
pub type RowId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

    pub fn is_truthy(&self) -> Tribool {
        match self {
            Value::Null => Tribool::Unknown,
            Value::Bool(b) => {
                if *b {
                    Tribool::True
                } else {
                    Tribool::False
                }
            }
            Value::Int(n) => {
                if *n != 0 {
                    Tribool::True
                } else {
                    Tribool::False
                }
            }
            _ => Tribool::True,
        }
    }

    pub fn coerce_to(&self, dt: &DataType) -> Result<Value> {
        match (self, dt) {
            (Value::Null, _) => Ok(Value::Null),
            (Value::Int(n), DataType::Int) | (Value::Int(n), DataType::BigInt) => {
                Ok(Value::Int(*n))
            }
            (Value::Int(n), DataType::Real) => Ok(Value::Real(*n as f64)),
            (Value::Int(n), DataType::Bool) => Ok(Value::Bool(*n != 0)),
            (Value::Int(n), DataType::Text) => Ok(Value::Text(n.to_string())),
            (Value::Real(r), DataType::Real) => Ok(Value::Real(*r)),
            (Value::Real(r), DataType::Int) | (Value::Real(r), DataType::BigInt) => {
                Ok(Value::Int(*r as i64))
            }
            (Value::Real(r), DataType::Text) => Ok(Value::Text(r.to_string())),
            (Value::Text(s), DataType::Text) => Ok(Value::Text(s.clone())),
            (Value::Text(s), DataType::Int) | (Value::Text(s), DataType::BigInt) => s
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| MiniDbError::Type(format!("cannot cast '{}' to INT", s))),
            (Value::Text(s), DataType::Real) => s
                .parse::<f64>()
                .map(Value::Real)
                .map_err(|_| MiniDbError::Type(format!("cannot cast '{}' to REAL", s))),
            (Value::Bool(b), DataType::Bool) => Ok(Value::Bool(*b)),
            (Value::Bool(b), DataType::Int) | (Value::Bool(b), DataType::BigInt) => {
                Ok(Value::Int(if *b { 1 } else { 0 }))
            }
            (Value::Bool(b), DataType::Text) => Ok(Value::Text(b.to_string())),
            _ => Err(MiniDbError::Type(format!(
                "cannot coerce {:?} to {:?}",
                self, dt
            ))),
        }
    }

    pub fn sql_eq(&self, other: &Value) -> Tribool {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Tribool::Unknown,
            (Value::Int(a), Value::Int(b)) => Tribool::from(a == b),
            (Value::Real(a), Value::Real(b)) => Tribool::from(a == b),
            (Value::Int(a), Value::Real(b)) => Tribool::from((*a as f64) == *b),
            (Value::Real(a), Value::Int(b)) => Tribool::from(*a == (*b as f64)),
            (Value::Text(a), Value::Text(b)) => Tribool::from(a == b),
            (Value::Bool(a), Value::Bool(b)) => Tribool::from(a == b),
            _ => Tribool::False,
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

    pub fn to_display_string(&self) -> String {
        match self {
            Value::Null => "NULL".to_string(),
            Value::Int(n) => n.to_string(),
            Value::Real(r) => format!("{}", r),
            Value::Text(s) => s.clone(),
            Value::Bool(b) => {
                if *b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
        }
    }

    pub fn add(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_add(*b))),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a + b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 + b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a + *b as f64)),
            _ => Err(MiniDbError::Type(format!(
                "cannot add {:?} and {:?}",
                self, other
            ))),
        }
    }

    pub fn sub(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_sub(*b))),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a - b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 - b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a - *b as f64)),
            _ => Err(MiniDbError::Type(format!(
                "cannot subtract {:?} from {:?}",
                other, self
            ))),
        }
    }

    pub fn mul(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_mul(*b))),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a * b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 * b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a * *b as f64)),
            _ => Err(MiniDbError::Type(format!(
                "cannot multiply {:?} and {:?}",
                self, other
            ))),
        }
    }

    pub fn div(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            (Value::Int(_), Value::Int(0)) => {
                Err(MiniDbError::Runtime("division by zero".to_string()))
            }
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
            (Value::Real(a), Value::Real(b)) => Ok(Value::Real(a / b)),
            (Value::Int(a), Value::Real(b)) => Ok(Value::Real(*a as f64 / b)),
            (Value::Real(a), Value::Int(b)) => Ok(Value::Real(a / *b as f64)),
            _ => Err(MiniDbError::Type(format!(
                "cannot divide {:?} by {:?}",
                self, other
            ))),
        }
    }

    pub fn negate(&self) -> Result<Value> {
        match self {
            Value::Null => Ok(Value::Null),
            Value::Int(n) => Ok(Value::Int(-n)),
            Value::Real(r) => Ok(Value::Real(-r)),
            _ => Err(MiniDbError::Type(format!("cannot negate {:?}", self))),
        }
    }

    pub fn concat(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            _ => {
                let a = self.to_display_string();
                let b = other.to_display_string();
                Ok(Value::Text(a + &b))
            }
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Value::Null => buf.push(0u8),
            Value::Int(n) => {
                buf.push(1u8);
                buf.extend_from_slice(&n.to_le_bytes());
            }
            Value::Real(r) => {
                buf.push(2u8);
                buf.extend_from_slice(&r.to_le_bytes());
            }
            Value::Text(s) => {
                buf.push(3u8);
                let bytes = s.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(bytes);
            }
            Value::Bool(b) => {
                buf.push(4u8);
                buf.push(if *b { 1u8 } else { 0u8 });
            }
        }
        buf
    }

    pub fn deserialize(data: &[u8]) -> Result<(Value, usize)> {
        if data.is_empty() {
            return Err(MiniDbError::Storage("empty value bytes".to_string()));
        }
        match data[0] {
            0 => Ok((Value::Null, 1)),
            1 => {
                if data.len() < 9 {
                    return Err(MiniDbError::Storage("truncated INT value".to_string()));
                }
                let n = i64::from_le_bytes(data[1..9].try_into().unwrap());
                Ok((Value::Int(n), 9))
            }
            2 => {
                if data.len() < 9 {
                    return Err(MiniDbError::Storage("truncated REAL value".to_string()));
                }
                let r = f64::from_le_bytes(data[1..9].try_into().unwrap());
                Ok((Value::Real(r), 9))
            }
            3 => {
                if data.len() < 5 {
                    return Err(MiniDbError::Storage("truncated TEXT value".to_string()));
                }
                let len = u32::from_le_bytes(data[1..5].try_into().unwrap()) as usize;
                if data.len() < 5 + len {
                    return Err(MiniDbError::Storage("truncated TEXT bytes".to_string()));
                }
                let s = String::from_utf8(data[5..5 + len].to_vec())
                    .map_err(|_| MiniDbError::Storage("invalid UTF-8 in TEXT".to_string()))?;
                Ok((Value::Text(s), 5 + len))
            }
            4 => {
                if data.len() < 2 {
                    return Err(MiniDbError::Storage("truncated BOOL value".to_string()));
                }
                Ok((Value::Bool(data[1] != 0), 2))
            }
            t => Err(MiniDbError::Storage(format!("unknown value tag: {}", t))),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Real(a), Value::Real(b)) => a == b,
            (Value::Text(a), Value::Text(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.sql_cmp(other)
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        // For storage purposes (BTree keys), we need a total order.
        // First compare by type tag, then by value within same type.
        fn type_tag(v: &Value) -> u8 {
            match v {
                Value::Null => 0,
                Value::Bool(_) => 1,
                Value::Int(_) => 2,
                Value::Real(_) => 3,
                Value::Text(_) => 4,
            }
        }
        let tt = type_tag(self).cmp(&type_tag(other));
        if tt != Ordering::Equal {
            return tt;
        }
        self.sql_cmp(other).unwrap_or(Ordering::Equal)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_display_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tribool {
    True,
    False,
    Unknown,
}

impl Tribool {
    pub fn from(b: bool) -> Self {
        if b {
            Tribool::True
        } else {
            Tribool::False
        }
    }

    pub fn and(self, other: Tribool) -> Tribool {
        match (self, other) {
            (Tribool::True, Tribool::True) => Tribool::True,
            (Tribool::False, _) | (_, Tribool::False) => Tribool::False,
            _ => Tribool::Unknown,
        }
    }

    pub fn or(self, other: Tribool) -> Tribool {
        match (self, other) {
            (Tribool::True, _) | (_, Tribool::True) => Tribool::True,
            (Tribool::False, Tribool::False) => Tribool::False,
            _ => Tribool::Unknown,
        }
    }

    pub fn not(self) -> Tribool {
        match self {
            Tribool::True => Tribool::False,
            Tribool::False => Tribool::True,
            Tribool::Unknown => Tribool::Unknown,
        }
    }

    pub fn is_true(self) -> bool {
        self == Tribool::True
    }
}

pub fn serialize_row(row: &Row) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&(row.len() as u32).to_le_bytes());
    for val in row {
        let v = val.serialize();
        buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
        buf.extend_from_slice(&v);
    }
    buf
}

pub fn deserialize_row(data: &[u8]) -> Result<Row> {
    if data.len() < 4 {
        return Err(MiniDbError::Storage("truncated row header".to_string()));
    }
    let ncols = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let mut offset = 4;
    let mut row = Vec::with_capacity(ncols);
    for _ in 0..ncols {
        if data.len() < offset + 4 {
            return Err(MiniDbError::Storage("truncated row col len".to_string()));
        }
        let vlen = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if data.len() < offset + vlen {
            return Err(MiniDbError::Storage("truncated row col data".to_string()));
        }
        let (val, _) = Value::deserialize(&data[offset..offset + vlen])?;
        offset += vlen;
        row.push(val);
    }
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_serialize_roundtrip() {
        let vals = vec![
            Value::Null,
            Value::Int(42),
            Value::Int(-100),
            Value::Real(3.14),
            Value::Text("hello world".to_string()),
            Value::Bool(true),
            Value::Bool(false),
        ];
        for v in &vals {
            let bytes = v.serialize();
            let (v2, _) = Value::deserialize(&bytes).unwrap();
            assert_eq!(*v, v2);
        }
    }

    #[test]
    fn test_row_serialize_roundtrip() {
        let row = vec![
            Value::Int(1),
            Value::Text("alice".to_string()),
            Value::Real(99.5),
            Value::Null,
        ];
        let bytes = serialize_row(&row);
        let row2 = deserialize_row(&bytes).unwrap();
        assert_eq!(row, row2);
    }

    #[test]
    fn test_tribool_logic() {
        assert_eq!(Tribool::True.and(Tribool::False), Tribool::False);
        assert_eq!(Tribool::True.or(Tribool::Unknown), Tribool::True);
        assert_eq!(Tribool::Unknown.not(), Tribool::Unknown);
        assert_eq!(Tribool::False.not(), Tribool::True);
    }

    #[test]
    fn test_sql_eq_null_propagation() {
        assert_eq!(Value::Null.sql_eq(&Value::Int(1)), Tribool::Unknown);
        assert_eq!(Value::Int(1).sql_eq(&Value::Null), Tribool::Unknown);
        assert_eq!(Value::Int(1).sql_eq(&Value::Int(1)), Tribool::True);
    }

    #[test]
    fn test_arithmetic() {
        assert_eq!(Value::Int(3).add(&Value::Int(4)).unwrap(), Value::Int(7));
        assert_eq!(
            Value::Real(1.5).add(&Value::Real(2.5)).unwrap(),
            Value::Real(4.0)
        );
        assert_eq!(Value::Null.add(&Value::Int(1)).unwrap(), Value::Null);
    }
}

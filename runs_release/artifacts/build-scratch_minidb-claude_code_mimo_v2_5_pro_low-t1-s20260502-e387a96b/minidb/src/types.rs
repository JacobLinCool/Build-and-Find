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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(i) => Some(*i as f64),
            Self::Real(r) => Some(*r),
            _ => None,
        }
    }

    pub fn cast_to(&self, ct: ColumnType) -> Result<Value> {
        match (self, ct) {
            (Self::Null, _) => Ok(Self::Null),
            (Self::Int(i), ColumnType::Int) => Ok(Self::Int(*i)),
            (Self::Int(i), ColumnType::Real) => Ok(Self::Real(*i as f64)),
            (Self::Int(i), ColumnType::Text) => Ok(Self::Text(i.to_string())),
            (Self::Int(0), ColumnType::Bool) => Ok(Self::Bool(false)),
            (Self::Int(_), ColumnType::Bool) => Ok(Self::Bool(true)),
            (Self::Real(r), ColumnType::Int) => Ok(Self::Int(*r as i64)),
            (Self::Real(r), ColumnType::Real) => Ok(Self::Real(*r)),
            (Self::Real(r), ColumnType::Text) => Ok(Self::Text(r.to_string())),
            (Self::Real(r), ColumnType::Bool) => Ok(Self::Bool(*r != 0.0)),
            (Self::Text(s), ColumnType::Int) => s
                .trim()
                .parse::<i64>()
                .map(Self::Int)
                .map_err(|_| MiniDbError::TypeMismatch(format!("cannot cast '{s}' to INT"))),
            (Self::Text(s), ColumnType::Real) => s
                .trim()
                .parse::<f64>()
                .map(Self::Real)
                .map_err(|_| MiniDbError::TypeMismatch(format!("cannot cast '{s}' to REAL"))),
            (Self::Text(s), ColumnType::Text) => Ok(Self::Text(s.clone())),
            (Self::Text(s), ColumnType::Bool) => match s.to_lowercase().as_str() {
                "true" | "1" | "yes" => Ok(Self::Bool(true)),
                "false" | "0" | "no" => Ok(Self::Bool(false)),
                _ => Err(MiniDbError::TypeMismatch(format!(
                    "cannot cast '{s}' to BOOL"
                ))),
            },
            (Self::Bool(b), ColumnType::Int) => Ok(Self::Int(if *b { 1 } else { 0 })),
            (Self::Bool(b), ColumnType::Real) => Ok(Self::Real(if *b { 1.0 } else { 0.0 })),
            (Self::Bool(b), ColumnType::Text) => Ok(Self::Text(b.to_string())),
            (Self::Bool(b), ColumnType::Bool) => Ok(Self::Bool(*b)),
        }
    }

    pub fn coerce_numeric(a: &Value, b: &Value) -> Option<(f64, f64)> {
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => Some((*x as f64, *y as f64)),
            (Value::Int(x), Value::Real(y)) => Some((*x as f64, *y)),
            (Value::Real(x), Value::Int(y)) => Some((*x, *y as f64)),
            (Value::Real(x), Value::Real(y)) => Some((*x, *y)),
            _ => None,
        }
    }

    pub fn add(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Self::Null, _) | (_, Self::Null) => Ok(Self::Null),
            (Self::Int(a), Self::Int(b)) => Ok(Self::Int(a.wrapping_add(*b))),
            _ => {
                if let Some((a, b)) = Self::coerce_numeric(self, other) {
                    Ok(Self::Real(a + b))
                } else {
                    Err(MiniDbError::TypeMismatch(format!(
                        "cannot add {} and {}",
                        self.type_name(),
                        other.type_name()
                    )))
                }
            }
        }
    }

    pub fn sub(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Self::Null, _) | (_, Self::Null) => Ok(Self::Null),
            (Self::Int(a), Self::Int(b)) => Ok(Self::Int(a.wrapping_sub(*b))),
            _ => {
                if let Some((a, b)) = Self::coerce_numeric(self, other) {
                    Ok(Self::Real(a - b))
                } else {
                    Err(MiniDbError::TypeMismatch(format!(
                        "cannot subtract {} from {}",
                        other.type_name(),
                        self.type_name()
                    )))
                }
            }
        }
    }

    pub fn mul(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Self::Null, _) | (_, Self::Null) => Ok(Self::Null),
            (Self::Int(a), Self::Int(b)) => Ok(Self::Int(a.wrapping_mul(*b))),
            _ => {
                if let Some((a, b)) = Self::coerce_numeric(self, other) {
                    Ok(Self::Real(a * b))
                } else {
                    Err(MiniDbError::TypeMismatch(format!(
                        "cannot multiply {} and {}",
                        self.type_name(),
                        other.type_name()
                    )))
                }
            }
        }
    }

    pub fn div(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Self::Null, _) | (_, Self::Null) => Ok(Self::Null),
            (Self::Int(_), Self::Int(0)) => {
                Err(MiniDbError::RuntimeError("division by zero".into()))
            }
            (Self::Int(a), Self::Int(b)) => Ok(Self::Int(a / b)),
            _ => {
                if let Some((a, b)) = Self::coerce_numeric(self, other) {
                    if b == 0.0 {
                        Err(MiniDbError::RuntimeError("division by zero".into()))
                    } else {
                        Ok(Self::Real(a / b))
                    }
                } else {
                    Err(MiniDbError::TypeMismatch(format!(
                        "cannot divide {} by {}",
                        self.type_name(),
                        other.type_name()
                    )))
                }
            }
        }
    }

    pub fn concat(&self, other: &Value) -> Result<Value> {
        match (self, other) {
            (Self::Null, _) | (_, Self::Null) => Ok(Self::Null),
            _ => Ok(Self::Text(format!("{self}{other}"))),
        }
    }

    pub fn negate(&self) -> Result<Value> {
        match self {
            Self::Null => Ok(Self::Null),
            Self::Int(i) => Ok(Self::Int(-i)),
            Self::Real(r) => Ok(Self::Real(-r)),
            _ => Err(MiniDbError::TypeMismatch(format!(
                "cannot negate {}",
                self.type_name()
            ))),
        }
    }

    pub fn not(&self) -> Value {
        match self {
            Self::Null => Self::Null,
            Self::Bool(b) => Self::Bool(!b),
            _ => Self::Null,
        }
    }

    pub fn and(&self, other: &Value) -> Value {
        match (self, other) {
            (Self::Bool(false), _) | (_, Self::Bool(false)) => Self::Bool(false),
            (Self::Bool(true), Self::Bool(true)) => Self::Bool(true),
            (Self::Null, Self::Bool(true)) | (Self::Bool(true), Self::Null) => Self::Null,
            _ => Self::Null,
        }
    }

    pub fn or(&self, other: &Value) -> Value {
        match (self, other) {
            (Self::Bool(true), _) | (_, Self::Bool(true)) => Self::Bool(true),
            (Self::Bool(false), Self::Bool(false)) => Self::Bool(false),
            (Self::Null, Self::Bool(false)) | (Self::Bool(false), Self::Null) => Self::Null,
            _ => Self::Null,
        }
    }

    pub fn compare(&self, other: &Value) -> Option<Ordering> {
        match (self, other) {
            (Self::Null, _) | (_, Self::Null) => None,
            (Self::Int(a), Self::Int(b)) => Some(a.cmp(b)),
            (Self::Real(a), Self::Real(b)) => a.partial_cmp(b),
            (Self::Int(a), Self::Real(b)) => (*a as f64).partial_cmp(b),
            (Self::Real(a), Self::Int(b)) => a.partial_cmp(&(*b as f64)),
            (Self::Text(a), Self::Text(b)) => Some(a.cmp(b)),
            (Self::Bool(a), Self::Bool(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }

    pub fn eq_check(&self, other: &Value) -> Value {
        match self.compare(other) {
            Some(Ordering::Equal) => Value::Bool(true),
            Some(_) => Value::Bool(false),
            None => Value::Null,
        }
    }

    pub fn neq_check(&self, other: &Value) -> Value {
        match self.compare(other) {
            Some(Ordering::Equal) => Value::Bool(false),
            Some(_) => Value::Bool(true),
            None => Value::Null,
        }
    }

    pub fn lt_check(&self, other: &Value) -> Value {
        match self.compare(other) {
            Some(Ordering::Less) => Value::Bool(true),
            Some(_) => Value::Bool(false),
            None => Value::Null,
        }
    }

    pub fn le_check(&self, other: &Value) -> Value {
        match self.compare(other) {
            Some(Ordering::Less | Ordering::Equal) => Value::Bool(true),
            Some(Ordering::Greater) => Value::Bool(false),
            None => Value::Null,
        }
    }

    pub fn gt_check(&self, other: &Value) -> Value {
        match self.compare(other) {
            Some(Ordering::Greater) => Value::Bool(true),
            Some(_) => Value::Bool(false),
            None => Value::Null,
        }
    }

    pub fn ge_check(&self, other: &Value) -> Value {
        match self.compare(other) {
            Some(Ordering::Greater | Ordering::Equal) => Value::Bool(true),
            Some(Ordering::Less) => Value::Bool(false),
            None => Value::Null,
        }
    }

    pub fn is_truthy(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            Self::Null => None,
            _ => Some(false),
        }
    }

    /// Serialize value to bytes for storage
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Null => vec![0],
            Self::Int(i) => {
                let mut v = vec![1u8];
                v.extend_from_slice(&i.to_le_bytes());
                v
            }
            Self::Real(r) => {
                let mut v = vec![2u8];
                v.extend_from_slice(&r.to_le_bytes());
                v
            }
            Self::Text(s) => {
                let bytes = s.as_bytes();
                let mut v = vec![3u8];
                v.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                v.extend_from_slice(bytes);
                v
            }
            Self::Bool(b) => vec![4, if *b { 1 } else { 0 }],
        }
    }

    /// Deserialize value from bytes
    pub fn from_bytes(data: &[u8]) -> Result<(Self, usize)> {
        if data.is_empty() {
            return Err(MiniDbError::Storage("empty value data".into()));
        }
        match data[0] {
            0 => Ok((Self::Null, 1)),
            1 => {
                if data.len() < 9 {
                    return Err(MiniDbError::Storage("truncated int value".into()));
                }
                let v = i64::from_le_bytes(data[1..9].try_into().unwrap());
                Ok((Self::Int(v), 9))
            }
            2 => {
                if data.len() < 9 {
                    return Err(MiniDbError::Storage("truncated real value".into()));
                }
                let v = f64::from_le_bytes(data[1..9].try_into().unwrap());
                Ok((Self::Real(v), 9))
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
                    .map_err(|_| MiniDbError::Storage("invalid UTF-8 in text value".into()))?;
                Ok((Self::Text(s), 5 + len))
            }
            4 => {
                if data.len() < 2 {
                    return Err(MiniDbError::Storage("truncated bool value".into()));
                }
                Ok((Self::Bool(data[1] != 0), 2))
            }
            tag => Err(MiniDbError::Storage(format!("unknown value tag: {tag}"))),
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

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            _ => self.compare(other) == Some(Ordering::Equal),
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

pub type Row = Vec<Value>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub check_expr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSchema {
    pub name: String,
    pub table_name: String,
    pub column_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_arithmetic() {
        let a = Value::Int(3);
        let b = Value::Int(4);
        assert_eq!(a.add(&b).unwrap(), Value::Int(7));
        assert_eq!(a.mul(&b).unwrap(), Value::Int(12));
        assert_eq!(b.sub(&a).unwrap(), Value::Int(1));
        assert_eq!(b.div(&a).unwrap(), Value::Int(1));
    }

    #[test]
    fn test_null_propagation() {
        let n = Value::Null;
        let a = Value::Int(5);
        assert!(n.add(&a).unwrap().is_null());
        assert!(a.mul(&n).unwrap().is_null());
        assert!(n.compare(&a).is_none());
    }

    #[test]
    fn test_three_valued_logic() {
        let t = Value::Bool(true);
        let f = Value::Bool(false);
        let n = Value::Null;
        assert_eq!(t.and(&f), Value::Bool(false));
        assert_eq!(t.or(&f), Value::Bool(true));
        assert!(n.and(&t).is_null());
        assert_eq!(n.or(&t), Value::Bool(true));
        assert_eq!(n.or(&f).is_null(), true);
    }

    #[test]
    fn test_value_serialization_roundtrip() {
        let values = vec![
            Value::Null,
            Value::Int(42),
            Value::Int(-1),
            Value::Real(3.14),
            Value::Text("hello world".into()),
            Value::Bool(true),
            Value::Bool(false),
        ];
        for v in values {
            let bytes = v.to_bytes();
            let (restored, consumed) = Value::from_bytes(&bytes).unwrap();
            assert_eq!(consumed, bytes.len());
            assert_eq!(v, restored);
        }
    }

    #[test]
    fn test_comparison() {
        assert!(Value::Int(3).lt_check(&Value::Int(5)) == Value::Bool(true));
        assert!(Value::Int(5).gt_check(&Value::Int(3)) == Value::Bool(true));
        assert!(Value::Int(3).eq_check(&Value::Int(3)) == Value::Bool(true));
        assert!(Value::Int(3).neq_check(&Value::Int(5)) == Value::Bool(true));
        assert!(Value::Null.eq_check(&Value::Int(3)).is_null());
    }

    #[test]
    fn test_concat() {
        let a = Value::Text("hello".into());
        let b = Value::Text(" world".into());
        assert_eq!(a.concat(&b).unwrap(), Value::Text("hello world".into()));
        let c = Value::Int(42);
        assert_eq!(a.concat(&c).unwrap(), Value::Text("hello42".into()));
    }

    #[test]
    fn test_coerce() {
        let i = Value::Int(3);
        let r = Value::Real(2.5);
        assert_eq!(i.add(&r).unwrap(), Value::Real(5.5));
    }
}

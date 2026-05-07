use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Eq)]
pub enum Value {
    Null,
    Int(i64),
    Real(RealOrd),
    Text(String),
    Bool(bool),
}

#[derive(Debug, Clone, Copy)]
pub struct RealOrd(pub f64);

impl PartialEq for RealOrd {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for RealOrd {}

impl Hash for RealOrd {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl Ord for RealOrd {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for RealOrd {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Value::Null => {}
            Value::Int(v) => v.hash(state),
            Value::Real(v) => v.hash(state),
            Value::Text(v) => v.hash(state),
            Value::Bool(v) => v.hash(state),
        }
    }
}

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
            (Value::Real(a), Value::Real(b)) => a.cmp(b),
            (Value::Text(a), Value::Text(b)) => a.cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            _ => {
                let tag = |v: &Value| match v {
                    Value::Null => 0u8,
                    Value::Int(_) => 1,
                    Value::Real(_) => 2,
                    Value::Text(_) => 3,
                    Value::Bool(_) => 4,
                };
                tag(self).cmp(&tag(other))
            }
        }
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Real(RealOrd(v))
    }
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    pub fn as_real(&self) -> Option<f64> {
        match self {
            Value::Real(v) => Some(v.0),
            Value::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Int(v) => Some(*v != 0),
            Value::Null => None,
            _ => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "NULL",
            Value::Int(_) => "INT",
            Value::Real(_) => "REAL",
            Value::Text(_) => "TEXT",
            Value::Bool(_) => "BOOL",
        }
    }

    pub fn sql_eq(&self, other: &Value) -> Option<bool> {
        if self.is_null() || other.is_null() {
            return None;
        }
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(a == b),
            (Value::Int(a), Value::Real(b)) => Some((*a as f64 - b.0).abs() < f64::EPSILON),
            (Value::Real(a), Value::Int(b)) => Some((a.0 - *b as f64).abs() < f64::EPSILON),
            (Value::Real(a), Value::Real(b)) => Some((a.0 - b.0).abs() < f64::EPSILON),
            (Value::Text(a), Value::Text(b)) => Some(a == b),
            (Value::Bool(a), Value::Bool(b)) => Some(a == b),
            _ => Some(false),
        }
    }

    pub fn sql_ne(&self, other: &Value) -> Option<bool> {
        self.sql_eq(other).map(|v| !v)
    }

    pub fn sql_lt(&self, other: &Value) -> Option<bool> {
        if self.is_null() || other.is_null() {
            return None;
        }
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(a < b),
            (Value::Int(a), Value::Real(b)) => Some((*a as f64) < b.0),
            (Value::Real(a), Value::Int(b)) => Some(a.0 < (*b as f64)),
            (Value::Real(a), Value::Real(b)) => Some(a.0 < b.0),
            (Value::Text(a), Value::Text(b)) => Some(a < b),
            (Value::Bool(a), Value::Bool(b)) => Some(a < b),
            _ => Some(false),
        }
    }

    pub fn sql_le(&self, other: &Value) -> Option<bool> {
        if self.is_null() || other.is_null() {
            return None;
        }
        self.sql_lt(other)
            .map(|v| v || self.sql_eq(other).unwrap_or(false))
    }

    pub fn sql_gt(&self, other: &Value) -> Option<bool> {
        other.sql_lt(self)
    }

    pub fn sql_ge(&self, other: &Value) -> Option<bool> {
        other.sql_le(self)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Int(v) => write!(f, "{}", v),
            Value::Real(v) => {
                if v.0.fract() == 0.0 && v.0.is_finite() {
                    write!(f, "{:.1}", v.0)
                } else {
                    write!(f, "{}", v.0)
                }
            }
            Value::Text(v) => write!(f, "{}", v),
            Value::Bool(v) => write!(f, "{}", if *v { "true" } else { "false" }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Int,
    Bigint,
    Real,
    Text,
    Bool,
}

impl ColumnType {
    pub fn default_value(&self) -> Value {
        match self {
            ColumnType::Int | ColumnType::Bigint => Value::Int(0),
            ColumnType::Real => Value::Real(RealOrd(0.0)),
            ColumnType::Text => Value::Text(String::new()),
            ColumnType::Bool => Value::Bool(false),
        }
    }

    pub fn coerce(&self, val: Value) -> Result<Value, String> {
        if val.is_null() {
            return Ok(Value::Null);
        }
        match self {
            ColumnType::Int | ColumnType::Bigint => match val {
                Value::Int(v) => Ok(Value::Int(v)),
                Value::Real(v) => Ok(Value::Int(v.0 as i64)),
                Value::Bool(v) => Ok(Value::Int(if v { 1 } else { 0 })),
                Value::Text(_) => Err("cannot convert TEXT to INT".into()),
                Value::Null => Ok(Value::Null),
            },
            ColumnType::Real => match val {
                Value::Real(v) => Ok(Value::Real(v)),
                Value::Int(v) => Ok(Value::Real(RealOrd(v as f64))),
                _ => Err(format!("cannot convert {} to REAL", val.type_name())),
            },
            ColumnType::Text => Ok(Value::Text(val.to_string())),
            ColumnType::Bool => match val {
                Value::Bool(v) => Ok(Value::Bool(v)),
                Value::Int(v) => Ok(Value::Bool(v != 0)),
                _ => Err(format!("cannot convert {} to BOOL", val.type_name())),
            },
        }
    }
}

impl fmt::Display for ColumnType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColumnType::Int => write!(f, "INT"),
            ColumnType::Bigint => write!(f, "BIGINT"),
            ColumnType::Real => write!(f, "REAL"),
            ColumnType::Text => write!(f, "TEXT"),
            ColumnType::Bool => write!(f, "BOOL"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    pub col_type: ColumnType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
    pub check: Option<String>,
}

impl ColumnDef {
    pub fn new(name: String, col_type: ColumnType) -> Self {
        ColumnDef {
            name,
            col_type,
            primary_key: false,
            not_null: false,
            unique: false,
            default: None,
            check: None,
        }
    }
}

pub type Row = Vec<Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowId(pub u64);

impl RowId {
    pub fn page_id(&self) -> u64 {
        self.0 / 1024
    }
    pub fn slot(&self) -> u16 {
        (self.0 % 1024) as u16
    }
    pub fn new(page_id: u64, slot: u16) -> Self {
        RowId(page_id * 1024 + slot as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_equality() {
        assert_eq!(Value::Int(1).sql_eq(&Value::Int(1)), Some(true));
        assert_eq!(Value::Int(1).sql_eq(&Value::Int(2)), Some(false));
        assert_eq!(Value::Int(1).sql_eq(&Value::Null), None);
        assert_eq!(Value::Null.sql_eq(&Value::Null), None);
    }

    #[test]
    fn test_value_ordering() {
        assert_eq!(Value::Int(1).sql_lt(&Value::Int(2)), Some(true));
        assert_eq!(Value::Int(2).sql_lt(&Value::Int(1)), Some(false));
        assert_eq!(
            Value::Text("a".into()).sql_lt(&Value::Text("b".into())),
            Some(true)
        );
    }

    #[test]
    fn test_type_coercion() {
        assert_eq!(
            ColumnType::Int.coerce(Value::Real(RealOrd(3.7))).unwrap(),
            Value::Int(3)
        );
        assert_eq!(
            ColumnType::Text.coerce(Value::Int(42)).unwrap(),
            Value::Text("42".into())
        );
        assert!(ColumnType::Int.coerce(Value::Text("abc".into())).is_err());
    }

    #[test]
    fn test_row_id() {
        let rid = RowId::new(5, 3);
        assert_eq!(rid.page_id(), 5);
        assert_eq!(rid.slot(), 3);
    }
}

use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum DataType {
    Int,
    Real,
    Text,
    Bool,
}

impl DataType {
    pub fn parse(s: &str) -> Option<DataType> {
        match s.to_ascii_uppercase().as_str() {
            "INT" | "INTEGER" | "BIGINT" => Some(DataType::Int),
            "REAL" | "FLOAT" | "DOUBLE" => Some(DataType::Real),
            "TEXT" | "VARCHAR" | "STRING" => Some(DataType::Text),
            "BOOL" | "BOOLEAN" => Some(DataType::Bool),
            _ => None,
        }
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataType::Int => f.write_str("INT"),
            DataType::Real => f.write_str("REAL"),
            DataType::Text => f.write_str("TEXT"),
            DataType::Bool => f.write_str("BOOL"),
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

    pub fn coerce_to(&self, ty: DataType) -> Result<Value> {
        if self.is_null() {
            return Ok(Value::Null);
        }
        Ok(match (self, ty) {
            (Value::Int(i), DataType::Int) => Value::Int(*i),
            (Value::Int(i), DataType::Real) => Value::Real(*i as f64),
            (Value::Real(r), DataType::Real) => Value::Real(*r),
            (Value::Real(r), DataType::Int) => Value::Int(*r as i64),
            (Value::Text(s), DataType::Text) => Value::Text(s.clone()),
            (Value::Bool(b), DataType::Bool) => Value::Bool(*b),
            (Value::Bool(b), DataType::Int) => Value::Int(if *b { 1 } else { 0 }),
            _ => {
                return Err(MiniDbError::Type(format!(
                    "cannot coerce {:?} to {}",
                    self, ty
                )))
            }
        })
    }

    /// SQL three-valued comparison. Returns None if either is NULL.
    pub fn cmp_sql(&self, other: &Value) -> Option<Ordering> {
        if self.is_null() || other.is_null() {
            return None;
        }
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
            (Value::Real(a), Value::Real(b)) => a.partial_cmp(b),
            (Value::Int(a), Value::Real(b)) => (*a as f64).partial_cmp(b),
            (Value::Real(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
            (Value::Text(a), Value::Text(b)) => Some(a.cmp(b)),
            (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }

    pub fn truthy(&self) -> Option<bool> {
        match self {
            Value::Null => None,
            Value::Bool(b) => Some(*b),
            Value::Int(i) => Some(*i != 0),
            Value::Real(r) => Some(*r != 0.0),
            Value::Text(s) => Some(!s.is_empty()),
        }
    }

    pub fn display_csv(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Int(i) => i.to_string(),
            Value::Real(r) => format_real(*r),
            Value::Text(s) => csv_quote(s),
            Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        }
    }

    pub fn display_json(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Int(i) => i.to_string(),
            Value::Real(r) => format_real(*r),
            Value::Text(s) => json_quote(s),
            Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        }
    }
}

fn format_real(r: f64) -> String {
    if r.is_nan() {
        "NaN".to_string()
    } else if r.is_infinite() {
        if r.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string()
    } else if r.fract() == 0.0 && r.abs() < 1e16 {
        format!("{:.1}", r)
    } else {
        format!("{}", r)
    }
}

fn csv_quote(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("NULL"),
            Value::Int(i) => write!(f, "{}", i),
            Value::Real(r) => f.write_str(&format_real(*r)),
            Value::Text(s) => f.write_str(s),
            Value::Bool(b) => f.write_str(if *b { "true" } else { "false" }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub data_type: DataType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub columns: Vec<Column>,
}

impl Schema {
    pub fn new(columns: Vec<Column>) -> Self {
        Schema { columns }
    }
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }
    pub fn names(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.name.clone()).collect()
    }
}

pub type Row = Vec<Value>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerce_int_to_real() {
        let v = Value::Int(3).coerce_to(DataType::Real).unwrap();
        assert!(matches!(v, Value::Real(x) if x == 3.0));
    }

    #[test]
    fn null_comparison_returns_none() {
        assert!(Value::Null.cmp_sql(&Value::Int(1)).is_none());
        assert!(Value::Int(1).cmp_sql(&Value::Null).is_none());
    }

    #[test]
    fn truthy_null_unknown() {
        assert!(Value::Null.truthy().is_none());
        assert_eq!(Value::Int(0).truthy(), Some(false));
        assert_eq!(Value::Int(2).truthy(), Some(true));
    }

    #[test]
    fn data_type_parse() {
        assert_eq!(DataType::parse("INT"), Some(DataType::Int));
        assert_eq!(DataType::parse("bigint"), Some(DataType::Int));
        assert_eq!(DataType::parse("nope"), None);
    }
}

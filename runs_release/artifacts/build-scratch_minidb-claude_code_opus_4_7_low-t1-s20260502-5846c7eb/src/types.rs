use crate::error::{MiniDbError, Result};
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataType {
    Int,
    Real,
    Text,
    Bool,
}

impl DataType {
    pub fn parse(name: &str) -> Result<DataType> {
        match name.to_ascii_uppercase().as_str() {
            "INT" | "INTEGER" | "BIGINT" => Ok(DataType::Int),
            "REAL" | "DOUBLE" | "FLOAT" => Ok(DataType::Real),
            "TEXT" | "STRING" | "VARCHAR" => Ok(DataType::Text),
            "BOOL" | "BOOLEAN" => Ok(DataType::Bool),
            other => Err(MiniDbError::Type(format!("unknown type {}", other))),
        }
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataType::Int => write!(f, "INT"),
            DataType::Real => write!(f, "REAL"),
            DataType::Text => write!(f, "TEXT"),
            DataType::Bool => write!(f, "BOOL"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
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

    /// Coerce a value into the column's declared type if possible.
    pub fn coerce(self, dt: DataType) -> Result<Value> {
        match (&self, dt) {
            (Value::Null, _) => Ok(Value::Null),
            (Value::Int(_), DataType::Int) => Ok(self),
            (Value::Int(i), DataType::Real) => Ok(Value::Real(*i as f64)),
            (Value::Real(_), DataType::Real) => Ok(self),
            (Value::Real(r), DataType::Int) => {
                if r.fract() == 0.0 {
                    Ok(Value::Int(*r as i64))
                } else {
                    Err(MiniDbError::Type(format!("cannot store real {} as INT", r)))
                }
            }
            (Value::Text(_), DataType::Text) => Ok(self),
            (Value::Bool(_), DataType::Bool) => Ok(self),
            (Value::Int(i), DataType::Bool) => Ok(Value::Bool(*i != 0)),
            (Value::Bool(b), DataType::Int) => Ok(Value::Int(if *b { 1 } else { 0 })),
            (v, t) => Err(MiniDbError::Type(format!("cannot coerce {:?} to {}", v, t))),
        }
    }

    pub fn to_display(&self) -> String {
        match self {
            Value::Null => "NULL".to_string(),
            Value::Int(i) => i.to_string(),
            Value::Real(r) => {
                if r.fract() == 0.0 && r.is_finite() {
                    format!("{:.1}", r)
                } else {
                    format!("{}", r)
                }
            }
            Value::Text(s) => s.clone(),
            Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        }
    }

    pub fn to_csv(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Text(s) => csv_quote(s),
            other => other.to_display(),
        }
    }

    pub fn to_json(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Int(i) => i.to_string(),
            Value::Real(r) => {
                if r.is_finite() {
                    format!("{}", r)
                } else {
                    "null".to_string()
                }
            }
            Value::Text(s) => json_quote(s),
            Value::Bool(b) => b.to_string(),
        }
    }
}

fn csv_quote(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
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

/// SQL three-valued boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriBool {
    True,
    False,
    Unknown,
}

impl TriBool {
    pub fn from_value(v: &Value) -> TriBool {
        match v {
            Value::Null => TriBool::Unknown,
            Value::Bool(true) => TriBool::True,
            Value::Bool(false) => TriBool::False,
            Value::Int(0) => TriBool::False,
            Value::Int(_) => TriBool::True,
            _ => TriBool::Unknown,
        }
    }

    pub fn and(self, other: TriBool) -> TriBool {
        match (self, other) {
            (TriBool::False, _) | (_, TriBool::False) => TriBool::False,
            (TriBool::True, TriBool::True) => TriBool::True,
            _ => TriBool::Unknown,
        }
    }

    pub fn or(self, other: TriBool) -> TriBool {
        match (self, other) {
            (TriBool::True, _) | (_, TriBool::True) => TriBool::True,
            (TriBool::False, TriBool::False) => TriBool::False,
            _ => TriBool::Unknown,
        }
    }

    pub fn not(self) -> TriBool {
        match self {
            TriBool::True => TriBool::False,
            TriBool::False => TriBool::True,
            TriBool::Unknown => TriBool::Unknown,
        }
    }

    pub fn is_true(self) -> bool {
        matches!(self, TriBool::True)
    }
}

/// Compare two SQL values using SQL semantics; NULL produces Unknown.
pub fn compare_values(a: &Value, b: &Value) -> Option<Ordering> {
    if a.is_null() || b.is_null() {
        return None;
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Real(x), Value::Real(y)) => x.partial_cmp(y),
        (Value::Int(x), Value::Real(y)) => (*x as f64).partial_cmp(y),
        (Value::Real(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
        (Value::Text(x), Value::Text(y)) => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row(pub Vec<Value>);

impl Row {
    pub fn new(values: Vec<Value>) -> Self {
        Row(values)
    }
    pub fn get(&self, i: usize) -> &Value {
        &self.0[i]
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tri_bool_logic() {
        assert_eq!(TriBool::True.and(TriBool::Unknown), TriBool::Unknown);
        assert_eq!(TriBool::False.and(TriBool::Unknown), TriBool::False);
        assert_eq!(TriBool::True.or(TriBool::Unknown), TriBool::True);
        assert_eq!(TriBool::Unknown.not(), TriBool::Unknown);
    }

    #[test]
    fn coerce_int_real() {
        let v = Value::Int(5).coerce(DataType::Real).unwrap();
        assert_eq!(v, Value::Real(5.0));
    }

    #[test]
    fn null_compare() {
        assert!(compare_values(&Value::Null, &Value::Int(1)).is_none());
        assert_eq!(
            compare_values(&Value::Int(1), &Value::Int(2)),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn csv_quote_special() {
        assert_eq!(Value::Text("a,b".into()).to_csv(), "\"a,b\"");
        assert_eq!(Value::Text("a\"b".into()).to_csv(), "\"a\"\"b\"");
        assert_eq!(Value::Text("plain".into()).to_csv(), "plain");
    }
}

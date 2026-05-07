use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColumnType {
    Int,
    Real,
    Text,
    Bool,
}

impl ColumnType {
    pub fn name(&self) -> &'static str {
        match self {
            ColumnType::Int => "INT",
            ColumnType::Real => "REAL",
            ColumnType::Text => "TEXT",
            ColumnType::Bool => "BOOL",
        }
    }
}

impl fmt::Display for ColumnType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
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
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn column_type(&self) -> Option<ColumnType> {
        match self {
            Value::Null => None,
            Value::Int(_) => Some(ColumnType::Int),
            Value::Real(_) => Some(ColumnType::Real),
            Value::Text(_) => Some(ColumnType::Text),
            Value::Bool(_) => Some(ColumnType::Bool),
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Real(v) => Some(*v as i64),
            Value::Bool(v) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_real(&self) -> Option<f64> {
        match self {
            Value::Real(v) => Some(*v),
            Value::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(v) => Some(*v),
            Value::Int(v) => Some(*v != 0),
            Value::Null => None,
            _ => None,
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
                    write!(f, "{:.1}", v)
                } else {
                    write!(f, "{}", v)
                }
            }
            Value::Text(v) => write!(f, "{}", v),
            Value::Bool(v) => write!(f, "{}", if *v { "true" } else { "false" }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub col_type: ColumnType,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<Value>,
    pub check: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub values: Vec<Value>,
}

impl Row {
    pub fn new(values: Vec<Value>) -> Self {
        Row { values }
    }

    pub fn is_null(&self) -> bool {
        self.values.iter().all(|v| v.is_null())
    }
}

#[derive(Debug, Clone)]
pub struct RowId {
    pub page_id: u64,
    pub slot: u16,
}

impl RowId {
    pub fn new(page_id: u64, slot: u16) -> Self {
        RowId { page_id, slot }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ColumnRef {
    pub table: Option<String>,
    pub name: String,
}

impl ColumnRef {
    pub fn new(name: String) -> Self {
        ColumnRef { table: None, name }
    }

    pub fn qualified(table: String, name: String) -> Self {
        ColumnRef {
            table: Some(table),
            name,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlLiteral {
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
    Null,
}

impl SqlLiteral {
    pub fn to_value(&self) -> Value {
        match self {
            SqlLiteral::Int(v) => Value::Int(*v),
            SqlLiteral::Real(v) => Value::Real(*v),
            SqlLiteral::Text(v) => Value::Text(v.clone()),
            SqlLiteral::Bool(v) => Value::Bool(*v),
            SqlLiteral::Null => Value::Null,
        }
    }
}

/// Tri-valued logic operations
pub fn sql_and(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(true), Some(true)) => Some(true),
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), None) | (None, Some(true)) => None,
        (None, None) => None,
    }
}

pub fn sql_or(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        (Some(false), None) | (None, Some(false)) => None,
        (None, None) => None,
    }
}

pub fn sql_not(a: Option<bool>) -> Option<bool> {
    a.map(|v| !v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_display() {
        assert_eq!(Value::Null.to_string(), "NULL");
        assert_eq!(Value::Int(42).to_string(), "42");
        assert_eq!(Value::Real(3.14).to_string(), "3.14");
        assert_eq!(Value::Text("hello".to_string()).to_string(), "hello");
        assert_eq!(Value::Bool(true).to_string(), "true");
    }

    #[test]
    fn test_value_conversions() {
        assert_eq!(Value::Int(42).as_int(), Some(42));
        assert_eq!(Value::Real(3.14).as_real(), Some(3.14));
        assert_eq!(Value::Text("hi".to_string()).as_text(), Some("hi"));
        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert!(Value::Null.as_int().is_none());
    }

    #[test]
    fn test_tri_valued_logic() {
        assert_eq!(sql_and(Some(true), Some(true)), Some(true));
        assert_eq!(sql_and(Some(true), Some(false)), Some(false));
        assert_eq!(sql_and(Some(true), None), None);
        assert_eq!(sql_and(None, None), None);

        assert_eq!(sql_or(Some(false), Some(false)), Some(false));
        assert_eq!(sql_or(Some(false), Some(true)), Some(true));
        assert_eq!(sql_or(Some(false), None), None);
        assert_eq!(sql_or(None, None), None);

        assert_eq!(sql_not(Some(true)), Some(false));
        assert_eq!(sql_not(None), None);
    }
}

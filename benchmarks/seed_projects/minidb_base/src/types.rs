//! Value model, column types, schema descriptor, and tri-valued logic.
//!
//! minidb stores dynamically-typed rows. A [`Value`] is the runtime
//! representation of a single SQL cell. A [`Schema`] is the static
//! description of a row: an ordered list of [`Column`]s with [`ColumnType`]s
//! and nullability flags.

use crate::error::{MiniDbError, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

/// The runtime value variant that flows through operators and the storage
/// layer. `Null` is first-class and participates in tri-valued logic.
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
            Value::Null => "NULL",
            Value::Int(_) => "INT",
            Value::Real(_) => "REAL",
            Value::Text(_) => "TEXT",
            Value::Bool(_) => "BOOL",
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Attempt to coerce into a boolean for `WHERE` evaluation. Returns
    /// `None` on NULL (the `UNKNOWN` case in tri-valued logic).
    pub fn to_bool(&self) -> Option<bool> {
        match self {
            Value::Null => None,
            Value::Bool(b) => Some(*b),
            Value::Int(i) => Some(*i != 0),
            Value::Real(r) => Some(*r != 0.0),
            Value::Text(s) => Some(!s.is_empty()),
        }
    }

    /// Coerce this value to the target column type, returning a fresh
    /// `Value`. Used during INSERT and UPDATE to normalise user input.
    pub fn coerce_to(&self, ty: ColumnType) -> Result<Value> {
        if self.is_null() {
            return Ok(Value::Null);
        }
        match (self, ty) {
            (Value::Int(_), ColumnType::Int) => Ok(self.clone()),
            (Value::Real(_), ColumnType::Real) => Ok(self.clone()),
            (Value::Text(_), ColumnType::Text) => Ok(self.clone()),
            (Value::Bool(_), ColumnType::Bool) => Ok(self.clone()),
            (Value::Int(i), ColumnType::Real) => Ok(Value::Real(*i as f64)),
            (Value::Int(i), ColumnType::Bool) => Ok(Value::Bool(*i != 0)),
            (Value::Bool(b), ColumnType::Int) => Ok(Value::Int(if *b { 1 } else { 0 })),
            _ => Err(MiniDbError::Type(format!(
                "cannot coerce {} into {:?}",
                self.type_name(),
                ty
            ))),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Real(r) => write!(f, "{r}"),
            Value::Text(s) => write!(f, "{s}"),
            Value::Bool(b) => write!(f, "{b}"),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        matches!(compare_values(self, other), Some(Ordering::Equal))
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        compare_values(self, other)
    }
}

/// Tri-valued comparison that returns `None` when either side is NULL.
pub fn compare_values(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Null, _) | (_, Value::Null) => None,
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Real(x), Value::Real(y)) => x.partial_cmp(y),
        (Value::Int(x), Value::Real(y)) => (*x as f64).partial_cmp(y),
        (Value::Real(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
        (Value::Text(x), Value::Text(y)) => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

/// Tri-valued logical AND.
pub fn tri_and(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

/// Tri-valued logical OR.
pub fn tri_or(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

/// Tri-valued logical NOT.
pub fn tri_not(a: Option<bool>) -> Option<bool> {
    a.map(|v| !v)
}

/// Static column type tag stored in the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    Int,
    Real,
    Text,
    Bool,
}

impl ColumnType {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_uppercase().as_str() {
            "INT" | "INTEGER" | "BIGINT" => Ok(ColumnType::Int),
            "REAL" | "DOUBLE" | "FLOAT" => Ok(ColumnType::Real),
            "TEXT" | "VARCHAR" | "STRING" => Ok(ColumnType::Text),
            "BOOL" | "BOOLEAN" => Ok(ColumnType::Bool),
            other => Err(MiniDbError::Parse(format!("unknown type `{other}`"))),
        }
    }
}

impl fmt::Display for ColumnType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColumnType::Int => write!(f, "INT"),
            ColumnType::Real => write!(f, "REAL"),
            ColumnType::Text => write!(f, "TEXT"),
            ColumnType::Bool => write!(f, "BOOL"),
        }
    }
}

/// Description of a single column within a [`Schema`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub ty: ColumnType,
    pub not_null: bool,
    pub unique: bool,
    pub primary_key: bool,
    pub default: Option<Value>,
}

impl Column {
    pub fn new(name: impl Into<String>, ty: ColumnType) -> Self {
        Self {
            name: name.into(),
            ty,
            not_null: false,
            unique: false,
            primary_key: false,
            default: None,
        }
    }
}

/// Ordered list of columns. Equivalent tuples share a schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Schema {
    pub columns: Vec<Column>,
}

impl Schema {
    pub fn new(columns: Vec<Column>) -> Self {
        Self { columns }
    }

    pub fn len(&self) -> usize {
        self.columns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    pub fn position(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn column(&self, idx: usize) -> Option<&Column> {
        self.columns.get(idx)
    }

    pub fn names(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.name.clone()).collect()
    }

    /// Concatenate two schemas. Used when planning JOINs.
    pub fn concat(&self, other: &Schema) -> Schema {
        let mut columns = self.columns.clone();
        columns.extend(other.columns.iter().cloned());
        Schema { columns }
    }
}

/// A runtime tuple: a row's worth of [`Value`]s.
pub type Row = Vec<Value>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tri_and_behaves() {
        assert_eq!(tri_and(Some(true), Some(true)), Some(true));
        assert_eq!(tri_and(Some(false), None), Some(false));
        assert_eq!(tri_and(None, Some(true)), None);
    }

    #[test]
    fn schema_position_case_insensitive() {
        let s = Schema::new(vec![Column::new("Foo", ColumnType::Int)]);
        assert_eq!(s.position("foo"), Some(0));
        assert_eq!(s.position("FOO"), Some(0));
    }

    #[test]
    fn column_type_parse() {
        assert_eq!(ColumnType::parse("int").unwrap(), ColumnType::Int);
        assert!(ColumnType::parse("zorb").is_err());
    }
}

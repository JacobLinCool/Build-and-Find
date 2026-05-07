use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    Int,
    BigInt,
    Real,
    Text,
    Bool,
}

impl ColumnType {
    pub fn name(self) -> &'static str {
        match self {
            ColumnType::Int | ColumnType::BigInt => "INT",
            ColumnType::Real => "REAL",
            ColumnType::Text => "TEXT",
            ColumnType::Bool => "BOOL",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnConstraint {
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Literal>,
}

impl ColumnConstraint {
    pub fn new() -> Self {
        Self {
            primary_key: false,
            not_null: false,
            unique: false,
            default: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub col_type: ColumnType,
    pub constraints: ColumnConstraint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TableConstraint {
    Check(crate::parser::ast::Expr),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
}

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
        use Value::*;
        match (self, other) {
            (Null, Null) => true,
            (Int(a), Int(b)) => a == b,
            (Real(a), Real(b)) => a.to_bits() == b.to_bits(),
            (Text(a), Text(b)) => a == b,
            (Bool(a), Bool(b)) => a == b,
            (Int(a), Real(b)) => (*a as f64).to_bits() == b.to_bits(),
            (Real(a), Int(b)) => a.to_bits() == (*b as f64).to_bits(),
            _ => false,
        }
    }
}

impl Eq for Value {}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Real(v) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::Real(v) => Some(*v),
            _ => None,
        }
    }

    pub fn from_literal(literal: &Literal) -> Self {
        match literal {
            Literal::Null => Value::Null,
            Literal::Int(v) => Value::Int(*v),
            Literal::Real(v) => Value::Real(*v),
            Literal::Text(v) => Value::Text(v.clone()),
            Literal::Bool(v) => Value::Bool(*v),
        }
    }

    pub fn kind_rank(&self) -> u8 {
        match self {
            Value::Null => 0,
            Value::Bool(_) => 1,
            Value::Int(_) => 2,
            Value::Real(_) => 3,
            Value::Text(_) => 4,
        }
    }

    pub fn total_cmp(&self, other: &Self) -> Ordering {
        use Value::*;
        match (self, other) {
            (Null, Null) => Ordering::Equal,
            (Bool(a), Bool(b)) => a.cmp(b),
            (Int(a), Int(b)) => a.cmp(b),
            (Real(a), Real(b)) => a.total_cmp(b),
            (Text(a), Text(b)) => a.cmp(b),
            (Int(a), Real(b)) => (*a as f64).total_cmp(b),
            (Real(a), Int(b)) => a.total_cmp(&(*b as f64)),
            _ => self.kind_rank().cmp(&other.kind_rank()),
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.total_cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        self.total_cmp(other)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Int(v) => write!(f, "{v}"),
            Value::Real(v) => write!(f, "{v}"),
            Value::Text(v) => write!(f, "{v}"),
            Value::Bool(v) => write!(f, "{v}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriBool {
    True,
    False,
    Null,
}

impl TriBool {
    pub fn from_value(value: &Value) -> Self {
        match value {
            Value::Bool(true) => TriBool::True,
            Value::Bool(false) => TriBool::False,
            Value::Null => TriBool::Null,
            _ => TriBool::Null,
        }
    }

    pub fn to_value(self) -> Value {
        match self {
            TriBool::True => Value::Bool(true),
            TriBool::False => Value::Bool(false),
            TriBool::Null => Value::Null,
        }
    }

    pub fn is_true(self) -> bool {
        matches!(self, TriBool::True)
    }

    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (TriBool::False, _) | (_, TriBool::False) => TriBool::False,
            (TriBool::True, TriBool::True) => TriBool::True,
            _ => TriBool::Null,
        }
    }

    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (TriBool::True, _) | (_, TriBool::True) => TriBool::True,
            (TriBool::False, TriBool::False) => TriBool::False,
            _ => TriBool::Null,
        }
    }

    pub fn not(self) -> Self {
        match self {
            TriBool::True => TriBool::False,
            TriBool::False => TriBool::True,
            TriBool::Null => TriBool::Null,
        }
    }
}

pub type Row = Vec<Value>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaColumn {
    pub table: Option<String>,
    pub name: String,
    pub col_type: ColumnType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultSet {
    pub columns: Vec<SchemaColumn>,
    pub rows: Vec<Row>,
}

impl ResultSet {
    pub fn empty() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
        }
    }
}

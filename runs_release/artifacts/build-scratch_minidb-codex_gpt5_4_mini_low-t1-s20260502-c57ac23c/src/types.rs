use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ColumnType {
    Int,
    BigInt,
    Real,
    Text,
    Bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Bool(bool),
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        use Value::*;
        match (self, other) {
            (Null, Null) => Ordering::Equal,
            (Null, _) => Ordering::Less,
            (_, Null) => Ordering::Greater,
            (Int(a), Int(b)) => a.cmp(b),
            (Real(a), Real(b)) => a.to_bits().cmp(&b.to_bits()),
            (Int(a), Real(b)) => (*a as f64).to_bits().cmp(&b.to_bits()),
            (Real(a), Int(b)) => a.to_bits().cmp(&(*b as f64).to_bits()),
            (Text(a), Text(b)) => a.cmp(b),
            (Bool(a), Bool(b)) => a.cmp(b),
            (Int(_), _) => Ordering::Less,
            (Real(_), Text(_) | Bool(_)) => Ordering::Less,
            (Text(_), Bool(_)) => Ordering::Less,
            (Text(_), Int(_) | Real(_)) => Ordering::Greater,
            (Bool(_), _) => Ordering::Greater,
        }
    }
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(v) = self {
            Some(*v)
        } else {
            None
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TruthValue {
    True,
    False,
    Unknown,
}

impl TruthValue {
    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (TruthValue::False, _) | (_, TruthValue::False) => TruthValue::False,
            (TruthValue::True, TruthValue::True) => TruthValue::True,
            _ => TruthValue::Unknown,
        }
    }
    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (TruthValue::True, _) | (_, TruthValue::True) => TruthValue::True,
            (TruthValue::False, TruthValue::False) => TruthValue::False,
            _ => TruthValue::Unknown,
        }
    }
    pub fn not(self) -> Self {
        match self {
            TruthValue::True => TruthValue::False,
            TruthValue::False => TruthValue::True,
            TruthValue::Unknown => TruthValue::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub table: Option<String>,
    pub name: String,
    pub col_type: ColumnType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row(pub Vec<Value>);

impl Row {
    pub fn get(&self, idx: usize) -> Option<&Value> {
        self.0.get(idx)
    }
}

pub fn compare_values(a: &Value, b: &Value) -> Option<Ordering> {
    use Value::*;
    match (a, b) {
        (Null, _) | (_, Null) => None,
        (Int(x), Int(y)) => Some(x.cmp(y)),
        (Real(x), Real(y)) => x.partial_cmp(y),
        (Int(x), Real(y)) => (*x as f64).partial_cmp(y),
        (Real(x), Int(y)) => x.partial_cmp(&(*y as f64)),
        (Text(x), Text(y)) => Some(x.cmp(y)),
        (Bool(x), Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn truth_tables_work() {
        assert_eq!(
            TruthValue::True.and(TruthValue::Unknown),
            TruthValue::Unknown
        );
        assert_eq!(
            TruthValue::False.or(TruthValue::Unknown),
            TruthValue::Unknown
        );
    }
}

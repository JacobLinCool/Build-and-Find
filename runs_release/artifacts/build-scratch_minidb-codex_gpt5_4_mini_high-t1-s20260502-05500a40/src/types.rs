use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    Int,
    BigInt,
    Real,
    Text,
    Bool,
}

impl ColumnType {
    pub fn canonical(&self) -> Self {
        match self {
            Self::BigInt => Self::Int,
            other => other.clone(),
        }
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
        matches!(self, Self::Null)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            Self::Null => None,
            _ => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "NULL",
            Self::Int(_) => "INT",
            Self::Real(_) => "REAL",
            Self::Text(_) => "TEXT",
            Self::Bool(_) => "BOOL",
        }
    }

    pub fn coerce_numeric(left: &Value, right: &Value) -> Option<(NumericValue, NumericValue)> {
        use NumericValue::*;
        match (left, right) {
            (Value::Int(a), Value::Int(b)) => Some((Int(*a), Int(*b))),
            (Value::Int(a), Value::Real(b)) => Some((Real(*a as f64), Real(*b))),
            (Value::Real(a), Value::Int(b)) => Some((Real(*a), Real(*b as f64))),
            (Value::Real(a), Value::Real(b)) => Some((Real(*a), Real(*b))),
            (Value::Null, _) | (_, Value::Null) => None,
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumericValue {
    Int(i64),
    Real(f64),
}

impl NumericValue {
    pub fn to_value(self) -> Value {
        match self {
            Self::Int(v) => Value::Int(v),
            Self::Real(v) => Value::Real(v),
        }
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Int(v) => write!(f, "{v}"),
            Value::Real(v) => write!(f, "{v}"),
            Value::Text(v) => write!(f, "{v}"),
            Value::Bool(v) => write!(f, "{v}"),
        }
    }
}

impl Eq for Value {}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        use Value::*;
        let rank = |v: &Value| match v {
            Null => 0,
            Bool(_) => 1,
            Int(_) => 2,
            Real(_) => 3,
            Text(_) => 4,
        };
        match rank(self).cmp(&rank(other)) {
            Ordering::Equal => match (self, other) {
                (Null, Null) => Ordering::Equal,
                (Bool(a), Bool(b)) => a.cmp(b),
                (Int(a), Int(b)) => a.cmp(b),
                (Real(a), Real(b)) => a.total_cmp(b),
                (Text(a), Text(b)) => a.cmp(b),
                _ => Ordering::Equal,
            },
            other => other,
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub col_type: ColumnType,
    pub not_null: bool,
    pub unique: bool,
    pub primary_key: bool,
    pub default: Option<Value>,
}

impl Column {
    pub fn new(name: impl Into<String>, col_type: ColumnType) -> Self {
        Self {
            name: name.into(),
            col_type,
            not_null: false,
            unique: false,
            primary_key: false,
            default: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaColumn {
    pub table: Option<String>,
    pub name: String,
    pub col_type: ColumnType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schema {
    pub columns: Vec<SchemaColumn>,
}

impl Schema {
    pub fn empty() -> Self {
        Self {
            columns: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.columns.len()
    }

    pub fn column_index(&self, table: Option<&str>, name: &str) -> Option<usize> {
        if let Some(table) = table {
            self.columns
                .iter()
                .position(|c| c.table.as_deref() == Some(table) && c.name == name)
        } else {
            let mut matches = self
                .columns
                .iter()
                .enumerate()
                .filter(|(_, c)| c.name == name)
                .map(|(idx, _)| idx);
            let first = matches.next();
            if matches.next().is_none() {
                first
            } else {
                None
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub values: Vec<Value>,
}

impl Row {
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bigint_is_alias_of_int() {
        assert_eq!(ColumnType::BigInt.canonical(), ColumnType::Int);
    }

    #[test]
    fn schema_resolves_unique_column() {
        let schema = Schema {
            columns: vec![
                SchemaColumn {
                    table: Some("t".into()),
                    name: "id".into(),
                    col_type: ColumnType::Int,
                },
                SchemaColumn {
                    table: Some("t".into()),
                    name: "name".into(),
                    col_type: ColumnType::Text,
                },
            ],
        };
        assert_eq!(schema.column_index(None, "name"), Some(1));
    }
}

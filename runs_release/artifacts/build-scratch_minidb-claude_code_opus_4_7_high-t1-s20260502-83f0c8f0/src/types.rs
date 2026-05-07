//! Core value, column, schema, and tri-valued logic primitives.
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
    pub fn parse(name: &str) -> Result<Self> {
        match name.to_ascii_uppercase().as_str() {
            "INT" | "INTEGER" | "BIGINT" => Ok(DataType::Int),
            "REAL" | "DOUBLE" | "FLOAT" => Ok(DataType::Real),
            "TEXT" | "VARCHAR" | "STRING" => Ok(DataType::Text),
            "BOOL" | "BOOLEAN" => Ok(DataType::Bool),
            other => Err(MiniDbError::ty(format!("unknown type: {}", other))),
        }
    }

    pub fn display(&self) -> &'static str {
        match self {
            DataType::Int => "INT",
            DataType::Real => "REAL",
            DataType::Text => "TEXT",
            DataType::Bool => "BOOL",
        }
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display())
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

    pub fn data_type(&self) -> Option<DataType> {
        match self {
            Value::Null => None,
            Value::Int(_) => Some(DataType::Int),
            Value::Real(_) => Some(DataType::Real),
            Value::Text(_) => Some(DataType::Text),
            Value::Bool(_) => Some(DataType::Bool),
        }
    }

    /// Cast a literal value to a target column type.
    pub fn coerce_to(self, target: DataType) -> Result<Value> {
        match (target, self) {
            (_, Value::Null) => Ok(Value::Null),
            (DataType::Int, Value::Int(n)) => Ok(Value::Int(n)),
            (DataType::Int, Value::Real(r)) => Ok(Value::Int(r as i64)),
            (DataType::Int, Value::Bool(b)) => Ok(Value::Int(if b { 1 } else { 0 })),
            (DataType::Int, Value::Text(_)) => Err(MiniDbError::ty("cannot coerce TEXT to INT")),
            (DataType::Real, Value::Real(r)) => Ok(Value::Real(r)),
            (DataType::Real, Value::Int(n)) => Ok(Value::Real(n as f64)),
            (DataType::Real, Value::Bool(b)) => Ok(Value::Real(if b { 1.0 } else { 0.0 })),
            (DataType::Real, Value::Text(_)) => Err(MiniDbError::ty("cannot coerce TEXT to REAL")),
            (DataType::Text, Value::Text(s)) => Ok(Value::Text(s)),
            (DataType::Text, v) => Ok(Value::Text(format!("{}", v))),
            (DataType::Bool, Value::Bool(b)) => Ok(Value::Bool(b)),
            (DataType::Bool, Value::Int(n)) => Ok(Value::Bool(n != 0)),
            (DataType::Bool, other) => Err(MiniDbError::ty(format!(
                "cannot coerce {} to BOOL",
                other.type_name()
            ))),
        }
    }

    /// Numeric promotion for arithmetic.
    pub fn promote_numeric(a: &Value, b: &Value) -> Result<(Value, Value)> {
        match (a, b) {
            (Value::Null, _) | (_, Value::Null) => Ok((Value::Null, Value::Null)),
            (Value::Int(x), Value::Int(y)) => Ok((Value::Int(*x), Value::Int(*y))),
            (Value::Real(x), Value::Real(y)) => Ok((Value::Real(*x), Value::Real(*y))),
            (Value::Int(x), Value::Real(y)) => Ok((Value::Real(*x as f64), Value::Real(*y))),
            (Value::Real(x), Value::Int(y)) => Ok((Value::Real(*x), Value::Real(*y as f64))),
            (Value::Bool(x), other) => {
                Value::promote_numeric(&Value::Int(if *x { 1 } else { 0 }), other)
            }
            (other, Value::Bool(y)) => {
                Value::promote_numeric(other, &Value::Int(if *y { 1 } else { 0 }))
            }
            _ => Err(MiniDbError::ty(format!(
                "non-numeric operand: {} {}",
                a.type_name(),
                b.type_name()
            ))),
        }
    }

    /// SQL three-valued comparison. Returns `None` if either side is NULL.
    pub fn cmp_sql(a: &Value, b: &Value) -> Option<Ordering> {
        match (a, b) {
            (Value::Null, _) | (_, Value::Null) => None,
            (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
            (Value::Real(x), Value::Real(y)) => x.partial_cmp(y),
            (Value::Int(x), Value::Real(y)) => (*x as f64).partial_cmp(y),
            (Value::Real(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
            (Value::Text(x), Value::Text(y)) => Some(x.cmp(y)),
            (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
            (Value::Bool(x), Value::Int(y)) => Some((if *x { 1i64 } else { 0 }).cmp(y)),
            (Value::Int(x), Value::Bool(y)) => Some(x.cmp(&(if *y { 1i64 } else { 0 }))),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("NULL"),
            Value::Int(n) => write!(f, "{}", n),
            Value::Real(r) => {
                if r.is_finite() && r.fract() == 0.0 && r.abs() < 1e16 {
                    write!(f, "{:.1}", r)
                } else {
                    write!(f, "{}", r)
                }
            }
            Value::Text(s) => f.write_str(s),
            Value::Bool(b) => f.write_str(if *b { "TRUE" } else { "FALSE" }),
        }
    }
}

/// Tri-valued logical truth value, used by predicate evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tri {
    True,
    False,
    Unknown,
}

impl Tri {
    pub fn from_bool(b: bool) -> Self {
        if b {
            Tri::True
        } else {
            Tri::False
        }
    }

    pub fn from_value(v: &Value) -> Self {
        match v {
            Value::Null => Tri::Unknown,
            Value::Bool(b) => Tri::from_bool(*b),
            Value::Int(n) => Tri::from_bool(*n != 0),
            Value::Real(r) => Tri::from_bool(*r != 0.0),
            Value::Text(s) => Tri::from_bool(!s.is_empty()),
        }
    }

    pub fn and(a: Tri, b: Tri) -> Tri {
        use Tri::*;
        match (a, b) {
            (False, _) | (_, False) => False,
            (True, True) => True,
            _ => Unknown,
        }
    }

    pub fn or(a: Tri, b: Tri) -> Tri {
        use Tri::*;
        match (a, b) {
            (True, _) | (_, True) => True,
            (False, False) => False,
            _ => Unknown,
        }
    }

    pub fn not(a: Tri) -> Tri {
        match a {
            Tri::True => Tri::False,
            Tri::False => Tri::True,
            Tri::Unknown => Tri::Unknown,
        }
    }

    pub fn is_true(self) -> bool {
        matches!(self, Tri::True)
    }

    pub fn to_value(self) -> Value {
        match self {
            Tri::True => Value::Bool(true),
            Tri::False => Value::Bool(false),
            Tri::Unknown => Value::Null,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

impl ColumnDef {
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        ColumnDef {
            name: name.into(),
            data_type,
            primary_key: false,
            not_null: false,
            unique: false,
            default: None,
        }
    }
}

/// A row schema (an ordered list of named, typed columns).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schema {
    pub columns: Vec<SchemaColumn>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaColumn {
    pub name: String,
    pub qualifier: Option<String>,
    pub data_type: Option<DataType>,
}

impl Schema {
    pub fn new(columns: Vec<SchemaColumn>) -> Self {
        Schema { columns }
    }

    pub fn empty() -> Self {
        Schema { columns: vec![] }
    }

    pub fn from_table(table: &str, defs: &[ColumnDef]) -> Self {
        Schema {
            columns: defs
                .iter()
                .map(|c| SchemaColumn {
                    name: c.name.clone(),
                    qualifier: Some(table.to_string()),
                    data_type: Some(c.data_type),
                })
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.columns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    /// Resolve `table.col` or `col` against this schema. Returns ordinal position.
    pub fn resolve(&self, qualifier: Option<&str>, name: &str) -> Result<usize> {
        let mut found = None;
        for (i, col) in self.columns.iter().enumerate() {
            if !col.name.eq_ignore_ascii_case(name) {
                continue;
            }
            if let Some(q) = qualifier {
                if let Some(cq) = &col.qualifier {
                    if !cq.eq_ignore_ascii_case(q) {
                        continue;
                    }
                } else {
                    continue;
                }
            }
            if found.is_some() {
                return Err(MiniDbError::ty(format!(
                    "ambiguous column reference: {}",
                    name
                )));
            }
            found = Some(i);
        }
        found.ok_or_else(|| {
            MiniDbError::ty(match qualifier {
                Some(q) => format!("unknown column: {}.{}", q, name),
                None => format!("unknown column: {}", name),
            })
        })
    }

    pub fn concat(left: &Schema, right: &Schema) -> Schema {
        let mut columns = left.columns.clone();
        columns.extend(right.columns.iter().cloned());
        Schema { columns }
    }
}

/// Row of values. Length matches the schema length.
pub type Row = Vec<Value>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_data_type() {
        assert_eq!(DataType::parse("int").unwrap(), DataType::Int);
        assert_eq!(DataType::parse("BIGINT").unwrap(), DataType::Int);
        assert_eq!(DataType::parse("real").unwrap(), DataType::Real);
        assert_eq!(DataType::parse("text").unwrap(), DataType::Text);
        assert_eq!(DataType::parse("BOOL").unwrap(), DataType::Bool);
        assert!(DataType::parse("blob").is_err());
    }

    #[test]
    fn cmp_with_null_is_unknown() {
        assert_eq!(Value::cmp_sql(&Value::Null, &Value::Int(1)), None);
        assert_eq!(
            Value::cmp_sql(&Value::Int(2), &Value::Int(3)),
            Some(Ordering::Less)
        );
        assert_eq!(
            Value::cmp_sql(&Value::Real(1.0), &Value::Int(1)),
            Some(Ordering::Equal)
        );
    }

    #[test]
    fn three_valued_logic() {
        assert_eq!(Tri::and(Tri::True, Tri::True), Tri::True);
        assert_eq!(Tri::and(Tri::True, Tri::Unknown), Tri::Unknown);
        assert_eq!(Tri::and(Tri::False, Tri::Unknown), Tri::False);
        assert_eq!(Tri::or(Tri::True, Tri::Unknown), Tri::True);
        assert_eq!(Tri::or(Tri::False, Tri::Unknown), Tri::Unknown);
        assert_eq!(Tri::not(Tri::Unknown), Tri::Unknown);
    }

    #[test]
    fn coerce_int_real() {
        let v = Value::Int(5).coerce_to(DataType::Real).unwrap();
        match v {
            Value::Real(r) => assert_eq!(r, 5.0),
            _ => panic!(),
        }
    }

    #[test]
    fn schema_resolve_qualified() {
        let s = Schema::new(vec![
            SchemaColumn {
                name: "id".into(),
                qualifier: Some("a".into()),
                data_type: Some(DataType::Int),
            },
            SchemaColumn {
                name: "id".into(),
                qualifier: Some("b".into()),
                data_type: Some(DataType::Int),
            },
        ]);
        assert_eq!(s.resolve(Some("a"), "id").unwrap(), 0);
        assert_eq!(s.resolve(Some("b"), "id").unwrap(), 1);
        assert!(s.resolve(None, "id").is_err());
    }
}

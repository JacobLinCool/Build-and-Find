use std::fmt;

/// SQL column types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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

    pub fn from_str(s: &str) -> Option<ColumnType> {
        match s.to_uppercase().as_str() {
            "INT" | "INTEGER" | "BIGINT" => Some(ColumnType::Int),
            "REAL" | "FLOAT" | "DOUBLE" => Some(ColumnType::Real),
            "TEXT" | "VARCHAR" | "STRING" => Some(ColumnType::Text),
            "BOOL" | "BOOLEAN" => Some(ColumnType::Bool),
            _ => None,
        }
    }
}

impl fmt::Display for ColumnType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// A database value - tri-valued (NULL propagates)
#[derive(Debug, Clone, PartialEq, PartialOrd)]
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
            Value::Bool(v) => Some(if *v { 1 } else { 0 }),
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
            _ => None,
        }
    }

    /// SQL three-valued logic AND
    pub fn and(a: &Value, b: &Value) -> Value {
        match (a.as_bool(), b.as_bool()) {
            (Some(true), Some(true)) => Value::Bool(true),
            (Some(false), _) | (_, Some(false)) => Value::Bool(false),
            _ => Value::Null,
        }
    }

    /// SQL three-valued logic OR
    pub fn or(a: &Value, b: &Value) -> Value {
        match (a.as_bool(), b.as_bool()) {
            (Some(true), _) | (_, Some(true)) => Value::Bool(true),
            (Some(false), Some(false)) => Value::Bool(false),
            _ => Value::Null,
        }
    }

    /// SQL three-valued logic NOT
    pub fn not(v: &Value) -> Value {
        match v.as_bool() {
            Some(b) => Value::Bool(!b),
            None => Value::Null,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Int(v) => write!(f, "{}", v),
            Value::Real(v) => {
                if v.fract() == 0.0 && v.is_finite() {
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

impl serde::Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        match self {
            Value::Null => {
                map.serialize_entry("t", "null")?;
                map.serialize_entry("v", &())?;
            }
            Value::Int(v) => {
                map.serialize_entry("t", "int")?;
                map.serialize_entry("v", v)?;
            }
            Value::Real(v) => {
                map.serialize_entry("t", "real")?;
                map.serialize_entry("v", v)?;
            }
            Value::Text(v) => {
                map.serialize_entry("t", "text")?;
                map.serialize_entry("v", v)?;
            }
            Value::Bool(v) => {
                map.serialize_entry("t", "bool")?;
                map.serialize_entry("v", v)?;
            }
        }
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for Value {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::MapAccess;
        use serde::de::Visitor;

        struct ValueVisitor;

        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = Value;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a Value map")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut type_str: Option<String> = None;
                let mut value: Option<serde_json::Value> = None;
                while let Some((k, v)) = map.next_entry::<String, serde_json::Value>()? {
                    if k == "t" {
                        type_str = v.as_str().map(|s| s.to_string());
                    } else if k == "v" {
                        value = Some(v);
                    }
                }
                match type_str.as_deref() {
                    Some("null") => Ok(Value::Null),
                    Some("int") => Ok(Value::Int(value.and_then(|v| v.as_i64()).unwrap_or(0))),
                    Some("real") => Ok(Value::Real(value.and_then(|v| v.as_f64()).unwrap_or(0.0))),
                    Some("text") => Ok(Value::Text(
                        value
                            .and_then(|v| v.as_str().map(|s| s.to_string()))
                            .unwrap_or_default(),
                    )),
                    Some("bool") => Ok(Value::Bool(
                        value.and_then(|v| v.as_bool()).unwrap_or(false),
                    )),
                    _ => Ok(Value::Null),
                }
            }
        }

        deserializer.deserialize_map(ValueVisitor)
    }
}

/// A row is a vector of values
pub type Row = Vec<Value>;

/// Column definition for a table
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub col_type: ColumnType,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

/// Table schema
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub row_count: u64,
}

impl TableSchema {
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn column_names(&self) -> Vec<&str> {
        self.columns.iter().map(|c| c.name.as_str()).collect()
    }
}

/// Index definition
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IndexDef {
    pub name: String,
    pub table: String,
    pub column: String,
}

/// Row ID type
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct RowId(pub u64);

impl fmt::Display for RowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_and() {
        assert_eq!(
            Value::and(&Value::Bool(true), &Value::Bool(true)),
            Value::Bool(true)
        );
        assert_eq!(
            Value::and(&Value::Bool(true), &Value::Bool(false)),
            Value::Bool(false)
        );
        assert_eq!(Value::and(&Value::Bool(true), &Value::Null), Value::Null);
        assert_eq!(Value::and(&Value::Null, &Value::Null), Value::Null);
    }

    #[test]
    fn test_value_or() {
        assert_eq!(
            Value::or(&Value::Bool(false), &Value::Bool(true)),
            Value::Bool(true)
        );
        assert_eq!(
            Value::or(&Value::Bool(false), &Value::Bool(false)),
            Value::Bool(false)
        );
        assert_eq!(Value::or(&Value::Bool(false), &Value::Null), Value::Null);
        assert_eq!(Value::or(&Value::Null, &Value::Null), Value::Null);
    }

    #[test]
    fn test_value_not() {
        assert_eq!(Value::not(&Value::Bool(true)), Value::Bool(false));
        assert_eq!(Value::not(&Value::Bool(false)), Value::Bool(true));
        assert_eq!(Value::not(&Value::Null), Value::Null);
    }

    #[test]
    fn test_column_type_from_str() {
        assert_eq!(ColumnType::from_str("int"), Some(ColumnType::Int));
        assert_eq!(ColumnType::from_str("BIGINT"), Some(ColumnType::Int));
        assert_eq!(ColumnType::from_str("TEXT"), Some(ColumnType::Text));
        assert_eq!(ColumnType::from_str("REAL"), Some(ColumnType::Real));
        assert_eq!(ColumnType::from_str("BOOL"), Some(ColumnType::Bool));
        assert_eq!(ColumnType::from_str("FLOAT"), Some(ColumnType::Real));
        assert_eq!(ColumnType::from_str("VARCHAR"), Some(ColumnType::Text));
        assert_eq!(ColumnType::from_str("FOO"), None);
    }

    #[test]
    fn test_value_conversions() {
        assert_eq!(Value::Int(42).as_int(), Some(42));
        assert_eq!(Value::Real(3.14).as_real(), Some(3.14));
        assert_eq!(Value::Text("hello".into()).as_text(), Some("hello"));
        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert!(Value::Null.is_null());
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Value::Null), "NULL");
        assert_eq!(format!("{}", Value::Int(42)), "42");
        assert_eq!(format!("{}", Value::Text("hello".into())), "hello");
        assert_eq!(format!("{}", Value::Bool(true)), "true");
    }
}

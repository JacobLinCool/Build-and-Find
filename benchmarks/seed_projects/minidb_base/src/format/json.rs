//! JSON output (one array of row-objects).

use crate::error::Result;
use crate::types::{Row, Schema, Value};
use serde_json::{json, Map};

pub fn render(schema: &Schema, rows: &[Row]) -> Result<String> {
    let headers = schema.names();
    let arr: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let mut obj = Map::new();
            for (i, h) in headers.iter().enumerate() {
                let v = r.get(i).cloned().unwrap_or(Value::Null);
                obj.insert(h.clone(), value_to_json(&v));
            }
            serde_json::Value::Object(obj)
        })
        .collect();
    Ok(serde_json::to_string_pretty(&serde_json::Value::Array(arr))?)
}

fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Int(i) => json!(i),
        Value::Real(r) => json!(r),
        Value::Text(s) => json!(s),
        Value::Bool(b) => json!(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, ColumnType};

    #[test]
    fn renders_json_objects() {
        let schema = Schema::new(vec![Column::new("n", ColumnType::Int)]);
        let rows = vec![vec![Value::Int(7)]];
        let out = render(&schema, &rows).unwrap();
        assert!(out.contains("\"n\""));
        assert!(out.contains('7'));
    }
}

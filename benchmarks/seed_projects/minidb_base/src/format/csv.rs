//! RFC 4180 CSV output.

use crate::types::{Row, Schema, Value};

pub fn render(schema: &Schema, rows: &[Row]) -> String {
    let mut out = String::new();
    let headers = schema.names();
    write_csv_line(&mut out, &headers);
    for row in rows {
        let cells: Vec<String> = row.iter().map(stringify).collect();
        write_csv_line(&mut out, &cells);
    }
    out
}

fn stringify(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn write_csv_line(buf: &mut String, cells: &[String]) {
    for (i, c) in cells.iter().enumerate() {
        if i > 0 {
            buf.push(',');
        }
        if needs_quoting(c) {
            buf.push('"');
            for ch in c.chars() {
                if ch == '"' {
                    buf.push_str("\"\"");
                } else {
                    buf.push(ch);
                }
            }
            buf.push('"');
        } else {
            buf.push_str(c);
        }
    }
    buf.push_str("\r\n");
}

fn needs_quoting(s: &str) -> bool {
    s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, ColumnType};

    #[test]
    fn quotes_fields_with_commas() {
        let schema = Schema::new(vec![Column::new("s", ColumnType::Text)]);
        let rows = vec![vec![Value::Text("a,b".into())]];
        let out = render(&schema, &rows);
        assert!(out.contains("\"a,b\""));
    }
}

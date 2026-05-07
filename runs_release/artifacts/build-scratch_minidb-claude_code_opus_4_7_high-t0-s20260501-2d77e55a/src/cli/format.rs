//! Result-set formatting: table, CSV (RFC 4180), and JSON.

use std::io::Write;

use crate::error::Result;
use crate::exec::{OperatorSchema, Row};
use crate::types::Value;

use super::OutputFormat;

pub struct Formatter {
    pub format: OutputFormat,
}

impl Formatter {
    pub fn new(format: OutputFormat) -> Self {
        Self { format }
    }

    pub fn write_query<W: Write>(
        &self,
        out: &mut W,
        schema: &OperatorSchema,
        rows: &[Row],
    ) -> Result<()> {
        match self.format {
            OutputFormat::Table => write_table(out, schema, rows),
            OutputFormat::Csv => write_csv(out, schema, rows),
            OutputFormat::Json => write_json(out, schema, rows),
        }
    }
}

fn cell_string(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        other => format!("{}", other),
    }
}

fn write_table<W: Write>(out: &mut W, schema: &OperatorSchema, rows: &[Row]) -> Result<()> {
    let header: Vec<String> = schema.column_names();
    let mut widths: Vec<usize> = header.iter().map(|s| s.chars().count()).collect();
    let mut data_rows: Vec<Vec<String>> = Vec::with_capacity(rows.len());
    for r in rows {
        let strs: Vec<String> = r.iter().map(cell_string).collect();
        for (i, s) in strs.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(s.chars().count());
            }
        }
        data_rows.push(strs);
    }
    let sep_line = {
        let mut s = String::from("+");
        for w in &widths {
            for _ in 0..(*w + 2) {
                s.push('-');
            }
            s.push('+');
        }
        s
    };
    writeln!(out, "{}", sep_line)?;
    write!(out, "|")?;
    for (i, h) in header.iter().enumerate() {
        write!(out, " {:width$} |", h, width = widths[i])?;
    }
    writeln!(out)?;
    writeln!(out, "{}", sep_line)?;
    for r in &data_rows {
        write!(out, "|")?;
        for (i, c) in r.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(0);
            write!(out, " {:width$} |", c, width = w)?;
        }
        writeln!(out)?;
    }
    writeln!(out, "{}", sep_line)?;
    Ok(())
}

fn write_csv<W: Write>(out: &mut W, schema: &OperatorSchema, rows: &[Row]) -> Result<()> {
    let header: Vec<String> = schema.column_names();
    let escaped: Vec<String> = header.iter().map(|s| csv_escape(s)).collect();
    writeln!(out, "{}", escaped.join(","))?;
    for r in rows {
        let cells: Vec<String> = r
            .iter()
            .map(|v| match v {
                Value::Null => String::new(),
                other => csv_escape(&format!("{}", other)),
            })
            .collect();
        writeln!(out, "{}", cells.join(","))?;
    }
    Ok(())
}

fn csv_escape(s: &str) -> String {
    let needs_quote = s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r');
    if needs_quote {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

fn write_json<W: Write>(out: &mut W, schema: &OperatorSchema, rows: &[Row]) -> Result<()> {
    let names = schema.column_names();
    write!(out, "[")?;
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            write!(out, ",")?;
        }
        write!(out, "{{")?;
        for (j, n) in names.iter().enumerate() {
            if j > 0 {
                write!(out, ",")?;
            }
            write!(out, "{}:", json_string(n))?;
            write!(out, "{}", json_value(r.get(j).unwrap_or(&Value::Null)))?;
        }
        write!(out, "}}")?;
    }
    writeln!(out, "]")?;
    Ok(())
}

fn json_string(s: &str) -> String {
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

fn json_value(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Int(i) => i.to_string(),
        Value::Real(r) => {
            if r.is_finite() {
                format!("{}", r)
            } else {
                "null".into()
            }
        }
        Value::Bool(b) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        Value::Text(s) => json_string(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::{ColumnRef, OperatorSchema};
    use crate::types::DataType;

    fn schema() -> OperatorSchema {
        OperatorSchema::new(vec![
            ColumnRef {
                table: None,
                name: "a".into(),
                dtype: Some(DataType::Int),
            },
            ColumnRef {
                table: None,
                name: "b".into(),
                dtype: Some(DataType::Text),
            },
        ])
    }

    #[test]
    fn csv_escapes_quotes_and_commas() {
        let s = schema();
        let rows = vec![vec![Value::Int(1), Value::Text("a,b\"c".into())]];
        let mut buf = Vec::new();
        Formatter::new(OutputFormat::Csv)
            .write_query(&mut buf, &s, &rows)
            .unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("\"a,b\"\"c\""));
    }

    #[test]
    fn json_roundtrips_through_serde() {
        let s = schema();
        let rows = vec![vec![Value::Int(1), Value::Text("hi".into())]];
        let mut buf = Vec::new();
        Formatter::new(OutputFormat::Json)
            .write_query(&mut buf, &s, &rows)
            .unwrap();
        let out = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["a"], serde_json::json!(1));
        assert_eq!(arr[0]["b"], serde_json::json!("hi"));
    }

    #[test]
    fn table_aligns_columns() {
        let s = schema();
        let rows = vec![
            vec![Value::Int(1), Value::Text("ok".into())],
            vec![Value::Int(123), Value::Text("longer".into())],
        ];
        let mut buf = Vec::new();
        Formatter::new(OutputFormat::Table)
            .write_query(&mut buf, &s, &rows)
            .unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("| 1   |"));
        assert!(out.contains("| 123 |"));
    }
}

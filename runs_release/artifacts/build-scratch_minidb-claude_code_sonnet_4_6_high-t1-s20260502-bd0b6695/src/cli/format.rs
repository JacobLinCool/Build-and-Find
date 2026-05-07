use crate::exec::QueryResult;
use crate::types::Value;
use std::fmt::Write as FmtWrite;

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "table" => Some(OutputFormat::Table),
            "csv" => Some(OutputFormat::Csv),
            "json" => Some(OutputFormat::Json),
            _ => None,
        }
    }
}

pub fn format_result(result: &QueryResult, fmt: &OutputFormat) -> String {
    match fmt {
        OutputFormat::Table => format_table(result),
        OutputFormat::Csv => format_csv(result),
        OutputFormat::Json => format_json(result),
    }
}

fn format_table(result: &QueryResult) -> String {
    if result.rows.is_empty() && result.schema.columns.is_empty() {
        return String::new();
    }
    if result.rows.is_empty() {
        // Just print header
        let headers: Vec<&str> = result
            .schema
            .columns
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        let widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
        return format_table_with_widths(&headers, &[], &widths);
    }

    let headers: Vec<&str> = result
        .schema
        .columns
        .iter()
        .map(|(n, _)| n.as_str())
        .collect();

    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    let cell_strings: Vec<Vec<String>> = result
        .rows
        .iter()
        .map(|row| row.iter().map(|v| value_display(v)).collect())
        .collect();

    for row_cells in &cell_strings {
        for (i, cell) in row_cells.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    format_table_with_widths(&headers, &cell_strings, &widths)
}

fn format_table_with_widths(headers: &[&str], rows: &[Vec<String>], widths: &[usize]) -> String {
    let mut out = String::new();

    // Top border
    let separator: String = widths
        .iter()
        .map(|w| "-".repeat(w + 2))
        .collect::<Vec<_>>()
        .join("+");
    writeln!(out, "+{}+", separator).ok();

    // Header row
    let header_row: String = headers
        .iter()
        .zip(widths.iter())
        .map(|(h, w)| format!(" {:width$} ", h, width = w))
        .collect::<Vec<_>>()
        .join("|");
    writeln!(out, "|{}|", header_row).ok();

    // Header separator
    writeln!(out, "+{}+", separator).ok();

    // Data rows
    for row in rows {
        let cells: Vec<String> = (0..headers.len())
            .map(|i| {
                let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
                let w = widths.get(i).copied().unwrap_or(0);
                format!(" {:width$} ", cell, width = w)
            })
            .collect();
        writeln!(out, "|{}|", cells.join("|")).ok();
    }

    // Bottom border
    writeln!(out, "+{}+", separator).ok();

    out
}

fn format_csv(result: &QueryResult) -> String {
    let mut out = String::new();

    // Header
    let headers: Vec<String> = result
        .schema
        .columns
        .iter()
        .map(|(n, _)| csv_quote(n))
        .collect();
    writeln!(out, "{}", headers.join(",")).ok();

    // Rows
    for row in &result.rows {
        let cells: Vec<String> = row.iter().map(|v| csv_quote_value(v)).collect();
        writeln!(out, "{}", cells.join(",")).ok();
    }

    out
}

fn csv_quote(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn csv_quote_value(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Text(s) => csv_quote(s),
        other => other.to_string(),
    }
}

fn format_json(result: &QueryResult) -> String {
    let mut out = String::new();
    out.push('[');

    for (ri, row) in result.rows.iter().enumerate() {
        if ri > 0 {
            out.push(',');
        }
        out.push('{');
        for (ci, (col_name, _)) in result.schema.columns.iter().enumerate() {
            if ci > 0 {
                out.push(',');
            }
            let key = serde_json::to_string(col_name).unwrap_or_default();
            let val = value_to_json(&row[ci]);
            write!(out, "{}:{}", key, val).ok();
        }
        out.push('}');
    }

    out.push(']');
    out.push('\n');
    out
}

fn value_to_json(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Int(i) => i.to_string(),
        Value::Real(r) => r.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Text(s) => serde_json::to_string(s).unwrap_or_else(|_| "null".to_string()),
    }
}

fn value_display(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::QueryResult;
    use crate::types::{DataType, Schema, Value};

    fn make_result() -> QueryResult {
        QueryResult {
            schema: Schema::new(vec![
                ("id".into(), DataType::Int),
                ("name".into(), DataType::Text),
            ]),
            rows: vec![
                vec![Value::Int(1), Value::Text("Alice".into())],
                vec![Value::Int(2), Value::Text("Bob".into())],
            ],
            affected_rows: None,
            message: None,
        }
    }

    #[test]
    fn test_table_format() {
        let result = make_result();
        let out = format_result(&result, &OutputFormat::Table);
        assert!(out.contains("Alice"));
        assert!(out.contains("id"));
        assert!(out.contains("|"));
    }

    #[test]
    fn test_csv_format() {
        let result = make_result();
        let out = format_result(&result, &OutputFormat::Csv);
        assert!(out.contains("id,name"));
        assert!(out.contains("1,Alice"));
    }

    #[test]
    fn test_json_format() {
        let result = make_result();
        let out = format_result(&result, &OutputFormat::Json);
        assert!(out.contains("\"id\""));
        assert!(out.contains("\"Alice\""));
        assert!(out.starts_with('['));
    }
}

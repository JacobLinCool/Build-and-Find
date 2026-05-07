//! Plain-ASCII bordered table renderer. Used as the default CLI output.

use crate::types::{Row, Schema};

pub fn render(schema: &Schema, rows: &[Row]) -> String {
    let headers: Vec<String> = schema.names();
    let col_count = headers.len();
    let mut widths = headers.iter().map(|h| h.len()).collect::<Vec<_>>();
    let rendered_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    for row in &rendered_rows {
        for (i, cell) in row.iter().enumerate().take(col_count) {
            if cell.len() > widths[i] {
                widths[i] = cell.len();
            }
        }
    }

    let mut out = String::new();
    let sep = make_sep(&widths);
    out.push_str(&sep);
    out.push('\n');
    out.push_str(&make_row(&headers, &widths));
    out.push('\n');
    out.push_str(&sep);
    out.push('\n');
    for row in &rendered_rows {
        out.push_str(&make_row(row, &widths));
        out.push('\n');
    }
    if !rendered_rows.is_empty() {
        out.push_str(&sep);
        out.push('\n');
    }
    out
}

fn make_sep(widths: &[usize]) -> String {
    let mut s = String::from("+");
    for w in widths {
        for _ in 0..(w + 2) {
            s.push('-');
        }
        s.push('+');
    }
    s
}

fn make_row(cells: &[String], widths: &[usize]) -> String {
    let mut s = String::from("|");
    for (cell, w) in cells.iter().zip(widths.iter()) {
        s.push(' ');
        s.push_str(cell);
        for _ in cell.len()..*w {
            s.push(' ');
        }
        s.push(' ');
        s.push('|');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, ColumnType, Value};

    #[test]
    fn renders_simple_table() {
        let schema = Schema::new(vec![
            Column::new("id", ColumnType::Int),
            Column::new("name", ColumnType::Text),
        ]);
        let rows = vec![vec![Value::Int(1), Value::Text("a".into())]];
        let out = render(&schema, &rows);
        assert!(out.contains("id"));
        assert!(out.contains("name"));
        assert!(out.contains('+'));
    }
}

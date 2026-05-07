//! Legacy Unicode box-drawing pretty-printer.
//!
//! This renderer emits tables decorated with the Unicode box-drawing code
//! points (U+2500 block). It predates the plain-ASCII `table` formatter
//! and is retained for backwards compatibility with older tooling that
//! scraped output by looking for these glyphs.
//!
//! New integrations should prefer one of the machine-readable formats
//! (`csv` or `json`).

use crate::types::{Row, Schema};

// Top-left / top / top-right / bottom-left / bottom / bottom-right / left-tee
// / right-tee / cross / horizontal / vertical.
const TL: char = '┌';
const TR: char = '┐';
const BL: char = '└';
const BR: char = '┘';
const T_DOWN: char = '┬';
const T_UP: char = '┴';
const T_RIGHT: char = '├';
const T_LEFT: char = '┤';
const CROSS: char = '┼';
const HORIZ: char = '─';
const VERT: char = '│';

pub fn render(schema: &Schema, rows: &[Row]) -> String {
    let headers = schema.names();
    let col_count = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    let rendered: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.iter().map(|v| v.to_string()).collect())
        .collect();
    for row in &rendered {
        for (i, cell) in row.iter().enumerate().take(col_count) {
            let w = cell.chars().count();
            if w > widths[i] {
                widths[i] = w;
            }
        }
    }

    let mut out = String::new();
    out.push_str(&border_line(&widths, TL, T_DOWN, TR));
    out.push('\n');
    out.push_str(&cells_line(&headers, &widths));
    out.push('\n');
    out.push_str(&border_line(&widths, T_RIGHT, CROSS, T_LEFT));
    out.push('\n');
    for row in &rendered {
        out.push_str(&cells_line(row, &widths));
        out.push('\n');
    }
    out.push_str(&border_line(&widths, BL, T_UP, BR));
    out.push('\n');
    out
}

fn border_line(widths: &[usize], left: char, mid: char, right: char) -> String {
    let mut s = String::new();
    s.push(left);
    for (i, w) in widths.iter().enumerate() {
        for _ in 0..(w + 2) {
            s.push(HORIZ);
        }
        if i + 1 == widths.len() {
            s.push(right);
        } else {
            s.push(mid);
        }
    }
    s
}

fn cells_line(cells: &[String], widths: &[usize]) -> String {
    let mut s = String::new();
    s.push(VERT);
    for (cell, w) in cells.iter().zip(widths.iter()) {
        s.push(' ');
        s.push_str(cell);
        let pad = *w - cell.chars().count();
        for _ in 0..pad {
            s.push(' ');
        }
        s.push(' ');
        s.push(VERT);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, ColumnType, Value};

    #[test]
    fn uses_box_drawing_chars() {
        let schema = Schema::new(vec![Column::new("n", ColumnType::Int)]);
        let rows = vec![vec![Value::Int(1)]];
        let out = render(&schema, &rows);
        assert!(out.contains('┌'));
        assert!(out.contains('└'));
        assert!(out.contains('│'));
    }
}

//! Output formatters.
//!
//! Each submodule renders a `(Schema, Vec<Row>)` pair into a string
//! suitable for stdout. Valid format ids are `table`, `csv`, `json`, and
//! the legacy `ascii-art`.

pub mod ascii_art;
pub mod csv;
pub mod json;
pub mod table;

use crate::error::{MiniDbError, Result};
use crate::types::{Row, Schema};

/// All supported output shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Plain ASCII bordered table. Default for interactive sessions.
    Table,
    /// RFC 4180 CSV with a header row.
    Csv,
    /// JSON array of row-objects.
    Json,
    /// Legacy Unicode box-drawing pretty-printer. Kept for backwards
    /// compatibility with older tooling.
    AsciiArt,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "csv" => Ok(OutputFormat::Csv),
            "json" => Ok(OutputFormat::Json),
            "ascii-art" | "ascii_art" | "asciiart" => Ok(OutputFormat::AsciiArt),
            other => Err(MiniDbError::InvalidArgument(format!(
                "unknown output format `{other}`"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OutputFormat::Table => "table",
            OutputFormat::Csv => "csv",
            OutputFormat::Json => "json",
            OutputFormat::AsciiArt => "ascii-art",
        }
    }
}

/// Render a result set according to `format`.
pub fn render(format: OutputFormat, schema: &Schema, rows: &[Row]) -> Result<String> {
    match format {
        OutputFormat::Table => Ok(table::render(schema, rows)),
        OutputFormat::Csv => Ok(csv::render(schema, rows)),
        OutputFormat::Json => json::render(schema, rows),
        OutputFormat::AsciiArt => Ok(ascii_art::render(schema, rows)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_formats() {
        assert_eq!(OutputFormat::parse("csv").unwrap(), OutputFormat::Csv);
        assert_eq!(
            OutputFormat::parse("ascii-art").unwrap(),
            OutputFormat::AsciiArt
        );
        assert!(OutputFormat::parse("yaml").is_err());
    }
}

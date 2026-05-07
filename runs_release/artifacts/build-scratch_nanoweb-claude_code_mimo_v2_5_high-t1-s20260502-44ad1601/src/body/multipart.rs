use crate::error::{NanowebError, Result};
use serde::de::DeserializeOwned;
use std::collections::HashMap;

pub struct MultipartParser;

#[derive(Debug, Clone)]
pub struct MultipartField {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

impl MultipartParser {
    pub fn parse_fields(content_type: &str, body: &[u8]) -> Result<Vec<MultipartField>> {
        let boundary = extract_boundary(content_type)
            .ok_or_else(|| NanowebError::Body("missing multipart boundary".into()))?;

        let boundary_bytes = format!("--{boundary}").into_bytes();
        let mut fields = Vec::new();
        let mut pos = 0;

        // Skip preamble to first boundary
        if let Some(idx) = find_subsequence(body, &boundary_bytes, pos) {
            pos = idx + boundary_bytes.len();
        } else {
            return Err(NanowebError::Body("no boundary found".into()));
        }

        loop {
            // Skip CRLF after boundary
            if pos < body.len() && body[pos] == b'\r' {
                pos += 1;
            }
            if pos < body.len() && body[pos] == b'\n' {
                pos += 1;
            }

            // Check for closing boundary
            if body[pos..].starts_with(b"--") {
                break;
            }

            // Parse headers until empty line
            let mut headers = HashMap::new();
            while pos < body.len() {
                let line_end = find_crlf(body, pos);
                if line_end == pos {
                    pos += 2; // skip \r\n
                    break;
                }
                let line = std::str::from_utf8(&body[pos..line_end])
                    .map_err(|e| NanowebError::Body(format!("invalid header: {e}")))?;
                if let Some((key, value)) = line.split_once(':') {
                    headers.insert(key.trim().to_lowercase(), value.trim().to_string());
                }
                pos = line_end + 2;
            }

            // Find next boundary
            let next_boundary = find_subsequence(body, &boundary_bytes, pos)
                .ok_or_else(|| NanowebError::Body("missing closing boundary".into()))?;

            let field_data = body[pos..next_boundary - 2].to_vec(); // -2 to remove trailing CRLF
            pos = next_boundary + boundary_bytes.len();

            let content_disposition = headers
                .get("content-disposition")
                .cloned()
                .unwrap_or_default();
            let name = extract_field_name(&content_disposition).unwrap_or_default();
            let filename = extract_filename(&content_disposition);
            let content_type = headers.get("content-type").cloned();

            fields.push(MultipartField {
                name,
                filename,
                content_type,
                data: field_data,
            });
        }

        Ok(fields)
    }

    pub fn parse_as_json<T: DeserializeOwned>(content_type: &str, body: &[u8]) -> Result<T> {
        let fields = Self::parse_fields(content_type, body)?;
        let map: HashMap<String, String> = fields
            .into_iter()
            .filter(|f| f.filename.is_none())
            .map(|f| {
                let value = String::from_utf8_lossy(&f.data).to_string();
                (f.name, value)
            })
            .collect();

        let json_val = serde_json::to_value(&map)
            .map_err(|e| NanowebError::Body(format!("serialization error: {e}")))?;
        serde_json::from_value(json_val)
            .map_err(|e| NanowebError::Body(format!("deserialization error: {e}")))
    }
}

fn extract_boundary(content_type: &str) -> Option<String> {
    for part in content_type.split(';') {
        let part = part.trim();
        if let Some(b) = part.strip_prefix("boundary=") {
            return Some(b.trim_matches('"').to_string());
        }
    }
    None
}

fn find_subsequence(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    haystack[start..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + start)
}

fn find_crlf(data: &[u8], start: usize) -> usize {
    for i in start..data.len().saturating_sub(1) {
        if data[i] == b'\r' && data[i + 1] == b'\n' {
            return i;
        }
    }
    data.len()
}

fn extract_field_name(disposition: &str) -> Option<String> {
    for part in disposition.split(';') {
        let part = part.trim();
        if let Some(name) = part.strip_prefix("name=") {
            return Some(name.trim_matches('"').to_string());
        }
    }
    None
}

fn extract_filename(disposition: &str) -> Option<String> {
    for part in disposition.split(';') {
        let part = part.trim();
        if let Some(name) = part.strip_prefix("filename=") {
            return Some(name.trim_matches('"').to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_boundary() {
        let ct = "multipart/form-data; boundary=----WebKitFormBoundary";
        assert_eq!(extract_boundary(ct).unwrap(), "----WebKitFormBoundary");
    }

    #[test]
    fn test_parse_multipart() {
        let ct = "multipart/form-data; boundary=abc";
        let body = b"--abc\r\nContent-Disposition: form-data; name=\"field1\"\r\n\r\nvalue1\r\n--abc--\r\n";
        let fields = MultipartParser::parse_fields(ct, body).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "field1");
        assert_eq!(fields[0].data, b"value1");
    }
}

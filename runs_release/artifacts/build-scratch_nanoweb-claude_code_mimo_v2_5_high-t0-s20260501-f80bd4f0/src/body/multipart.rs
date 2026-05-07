use crate::error::{NanowebError, Result};
use serde::de::DeserializeOwned;
use std::collections::HashMap;

pub fn parse_multipart<T: DeserializeOwned>(body: &[u8], boundary: &str) -> Result<T> {
    let fields = parse_multipart_fields(body, boundary)?;
    let json_val = serde_json::to_value(&fields)
        .map_err(|e| NanowebError::Parse(format!("Conversion error: {e}")))?;
    serde_json::from_value(json_val).map_err(|e| NanowebError::Parse(e.to_string()))
}

fn parse_multipart_fields(body: &[u8], boundary: &str) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    let delimiter = format!("--{boundary}");

    // Find all occurrences of the delimiter
    let mut parts = Vec::new();
    let mut start = 0;

    while let Some(pos) = find_subsequence(body[start..].as_ref(), delimiter.as_bytes()) {
        let abs_pos = start + pos;
        let delim_end = abs_pos + delimiter.len();

        // Skip past the delimiter and any CRLF
        let content_start =
            if body.get(delim_end) == Some(&b'\r') && body.get(delim_end + 1) == Some(&b'\n') {
                delim_end + 2
            } else if body.get(delim_end) == Some(&b'-') {
                // End marker
                break;
            } else {
                delim_end
            };

        // Find next delimiter
        if let Some(next_pos) =
            find_subsequence(body[content_start..].as_ref(), delimiter.as_bytes())
        {
            let content_end = content_start + next_pos;
            // Strip trailing CRLF
            let content = &body[content_start..content_end];
            let content = content.strip_suffix(b"\r\n").unwrap_or(content);
            parts.push(content);
            start = content_start + next_pos;
        } else {
            break;
        }
    }

    for part in &parts {
        let header_end = find_header_end(part);
        if let Some(end) = header_end {
            let header_bytes = &part[..end];
            let body_bytes = &part[end + 4..]; // skip \r\n\r\n

            let headers_str = std::str::from_utf8(header_bytes).unwrap_or("");
            let mut name = String::new();

            for line in headers_str.lines() {
                if let Some(val) = line.strip_prefix("Content-Disposition:") {
                    if let Some(n) = extract_form_field(val) {
                        name = n;
                    }
                }
            }

            if !name.is_empty() {
                let body_clean = body_bytes.strip_suffix(b"\r\n").unwrap_or(body_bytes);
                let value = String::from_utf8_lossy(body_clean).to_string();
                map.insert(name, value);
            }
        }
    }

    Ok(map)
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w == b"\r\n\r\n")
}

fn extract_form_field(s: &str) -> Option<String> {
    for part in s.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("name=\"") {
            return val.strip_suffix('"').map(|v| v.to_string());
        }
        if let Some(val) = part.strip_prefix("name=") {
            return Some(val.trim_matches('"').to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestForm {
        field1: String,
        field2: String,
    }

    #[test]
    fn test_extract_form_field() {
        assert_eq!(
            extract_form_field("form-data; name=\"field1\""),
            Some("field1".into())
        );
        assert_eq!(
            extract_form_field("form-data; name=field2"),
            Some("field2".into())
        );
    }

    #[test]
    fn test_find_subsequence() {
        let data = b"hello world";
        assert_eq!(find_subsequence(data, b"world"), Some(6));
        assert_eq!(find_subsequence(data, b"xyz"), None);
    }
}

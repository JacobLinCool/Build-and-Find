//! Minimal multipart/form-data parser.

use crate::error::{NanowebError, Result};

#[derive(Debug, Clone)]
pub struct Part {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub is_file: bool,
}

pub fn extract_boundary(content_type: &str) -> Option<String> {
    for piece in content_type.split(';') {
        let p = piece.trim();
        if let Some(rest) = p.strip_prefix("boundary=") {
            let trimmed = rest.trim_matches('"');
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Parse a complete multipart body using the given boundary.
pub fn parse(body: &[u8], boundary: &str) -> Result<Vec<Part>> {
    let dash = format!("--{}", boundary);
    let dash_bytes = dash.as_bytes();
    let mut parts = Vec::new();

    // Find first opening boundary.
    let first = find(body, dash_bytes)
        .ok_or_else(|| NanowebError::Body("multipart: no opening boundary".into()))?;
    let mut idx = first + dash_bytes.len();

    loop {
        // Either the close marker or CRLF starting a new part.
        if body[idx..].starts_with(b"--") {
            return Ok(parts);
        }
        if !body[idx..].starts_with(b"\r\n") {
            return Err(NanowebError::Body(
                "multipart: malformed boundary line".into(),
            ));
        }
        idx += 2;

        // Find header terminator (\r\n\r\n).
        let head_end = find(&body[idx..], b"\r\n\r\n")
            .ok_or_else(|| NanowebError::Body("multipart: missing header terminator".into()))?;
        let head = std::str::from_utf8(&body[idx..idx + head_end])
            .map_err(|_| NanowebError::Body("multipart: invalid header utf-8".into()))?;
        let (name, filename, ct) = parse_part_headers(head)?;
        idx += head_end + 4;

        // Body extends until \r\n--boundary.
        let mut needle = Vec::with_capacity(2 + dash_bytes.len());
        needle.extend_from_slice(b"\r\n");
        needle.extend_from_slice(dash_bytes);
        let rel = find(&body[idx..], &needle)
            .ok_or_else(|| NanowebError::Body("multipart: no closing boundary for part".into()))?;
        let part_body = body[idx..idx + rel].to_vec();
        let is_file = filename.is_some();
        parts.push(Part {
            name,
            filename,
            content_type: ct,
            body: part_body,
            is_file,
        });
        idx += rel + needle.len();
    }
}

fn parse_part_headers(head: &str) -> Result<(String, Option<String>, Option<String>)> {
    let mut name = None;
    let mut filename = None;
    let mut content_type = None;
    for line in head.split("\r\n") {
        if line.is_empty() {
            continue;
        }
        let colon = line
            .find(':')
            .ok_or_else(|| NanowebError::Body("multipart: bad header".into()))?;
        let header_name = line[..colon].trim();
        let value = line[colon + 1..].trim();
        if header_name.eq_ignore_ascii_case("content-disposition") {
            for piece in value.split(';') {
                let p = piece.trim();
                if let Some(rest) = p.strip_prefix("name=") {
                    name = Some(rest.trim_matches('"').to_string());
                } else if let Some(rest) = p.strip_prefix("filename=") {
                    filename = Some(rest.trim_matches('"').to_string());
                }
            }
        } else if header_name.eq_ignore_ascii_case("content-type") {
            content_type = Some(value.to_string());
        }
    }
    let name = name.ok_or_else(|| NanowebError::Body("multipart: missing name".into()))?;
    Ok((name, filename, content_type))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    for i in 0..=haystack.len() - needle.len() {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_text_fields() {
        let boundary = "X";
        let body = b"\
--X\r\n\
Content-Disposition: form-data; name=\"a\"\r\n\
\r\n\
1\r\n\
--X\r\n\
Content-Disposition: form-data; name=\"b\"\r\n\
\r\n\
hello\r\n\
--X--\r\n";
        let parts = parse(body, boundary).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name, "a");
        assert_eq!(parts[0].body, b"1");
        assert_eq!(parts[1].name, "b");
        assert_eq!(parts[1].body, b"hello");
    }

    #[test]
    fn parses_file_field() {
        let boundary = "Y";
        let body = b"\
--Y\r\n\
Content-Disposition: form-data; name=\"file\"; filename=\"a.txt\"\r\n\
Content-Type: text/plain\r\n\
\r\n\
hello\r\n\
--Y--\r\n";
        let parts = parse(body, boundary).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].filename.as_deref(), Some("a.txt"));
        assert!(parts[0].is_file);
        assert_eq!(parts[0].body, b"hello");
    }

    #[test]
    fn boundary_extraction() {
        let b = extract_boundary("multipart/form-data; boundary=ABC123");
        assert_eq!(b.as_deref(), Some("ABC123"));
        let b = extract_boundary("multipart/form-data; boundary=\"X-Y\"");
        assert_eq!(b.as_deref(), Some("X-Y"));
    }
}

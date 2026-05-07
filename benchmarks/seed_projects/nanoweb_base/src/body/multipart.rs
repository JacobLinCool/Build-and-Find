//! Minimal `multipart/form-data` parser.
//!
//! Accepts parts with `Content-Disposition: form-data; name="..."` and
//! produces a `MultipartField` with name, optional filename, optional
//! content-type, and the raw bytes. Good enough for small uploads in
//! the seed project; not optimised for huge files.

use crate::error::NanowebError;
use crate::http::types::Request;

#[derive(Debug, Clone)]
pub struct MultipartField {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

/// Parse a multipart/form-data body using the boundary from the
/// request's Content-Type header.
pub fn parse_multipart(req: &Request) -> Result<Vec<MultipartField>, NanowebError> {
    let ct = req
        .headers
        .get("content-type")
        .ok_or_else(|| NanowebError::Body("missing content-type for multipart".into()))?;
    let boundary = extract_boundary(ct)
        .ok_or_else(|| NanowebError::Body("multipart boundary missing".into()))?;
    parse_bytes(&req.body, &boundary)
}

fn extract_boundary(ct: &str) -> Option<String> {
    for part in ct.split(';') {
        let p = part.trim();
        if let Some(rest) = p.strip_prefix("boundary=") {
            let r = rest.trim_matches('"');
            return Some(r.to_string());
        }
    }
    None
}

fn parse_bytes(body: &[u8], boundary: &str) -> Result<Vec<MultipartField>, NanowebError> {
    let delim = format!("--{}", boundary);
    let delim_bytes = delim.as_bytes();
    let mut fields = Vec::new();

    // Skip any preamble.
    let first = find_subslice(body, delim_bytes)
        .ok_or_else(|| NanowebError::Body("opening boundary not found".into()))?;
    let mut i = first + delim_bytes.len();

    loop {
        // Check for the terminating "--".
        if body.get(i..i + 2) == Some(b"--") {
            break;
        }
        // Skip CRLF after boundary.
        if body.get(i..i + 2) == Some(b"\r\n") {
            i += 2;
        }
        // Find end of headers.
        let head_end = find_subslice(&body[i..], b"\r\n\r\n")
            .ok_or_else(|| NanowebError::Body("missing part headers terminator".into()))?;
        let head = std::str::from_utf8(&body[i..i + head_end])
            .map_err(|_| NanowebError::Body("multipart part headers not utf-8".into()))?;
        let (name, filename, content_type) = parse_part_headers(head)?;
        i += head_end + 4;

        // Locate the next boundary.
        let next = find_subslice(&body[i..], delim_bytes)
            .ok_or_else(|| NanowebError::Body("truncated multipart body".into()))?;
        let data_end = i + next;
        let mut payload_end = data_end;
        if body.get(payload_end.saturating_sub(2)..payload_end) == Some(b"\r\n") {
            payload_end -= 2;
        }
        fields.push(MultipartField {
            name,
            filename,
            content_type,
            bytes: body[i..payload_end].to_vec(),
        });
        i = data_end + delim_bytes.len();
    }

    Ok(fields)
}

fn parse_part_headers(head: &str) -> Result<(String, Option<String>, Option<String>), NanowebError> {
    let mut name = None;
    let mut filename = None;
    let mut content_type = None;
    for line in head.split("\r\n") {
        if line.is_empty() {
            continue;
        }
        let colon = match line.find(':') {
            Some(i) => i,
            None => return Err(NanowebError::Body("malformed part header".into())),
        };
        let key = line[..colon].trim().to_ascii_lowercase();
        let value = line[colon + 1..].trim();
        match key.as_str() {
            "content-disposition" => {
                for bit in value.split(';') {
                    let bit = bit.trim();
                    if let Some(n) = bit.strip_prefix("name=") {
                        name = Some(n.trim_matches('"').to_string());
                    } else if let Some(f) = bit.strip_prefix("filename=") {
                        filename = Some(f.trim_matches('"').to_string());
                    }
                }
            }
            "content-type" => content_type = Some(value.to_string()),
            _ => {}
        }
    }
    let name = name.ok_or_else(|| NanowebError::Body("part without name".into()))?;
    Ok((name, filename, content_type))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::Method;

    #[test]
    fn parses_two_fields() {
        let boundary = "X";
        let body = b"--X\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nhello\r\n--X\r\nContent-Disposition: form-data; name=\"b\"; filename=\"f.txt\"\r\nContent-Type: text/plain\r\n\r\nworld\r\n--X--\r\n";
        let mut req = Request::new(Method::Post, "/");
        req.headers
            .set("content-type", format!("multipart/form-data; boundary={}", boundary));
        req.body = body.to_vec();
        let fields = parse_multipart(&req).unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "a");
        assert_eq!(fields[0].bytes, b"hello");
        assert_eq!(fields[1].name, "b");
        assert_eq!(fields[1].filename.as_deref(), Some("f.txt"));
        assert_eq!(fields[1].content_type.as_deref(), Some("text/plain"));
    }
}

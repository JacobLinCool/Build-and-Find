use bytes::BytesMut;
use std::str;

use crate::error::{NanowebError, Result};
use crate::http::types::{Headers, Method, Request};

const MAX_REQUEST_LINE: usize = 8192;
const MAX_HEADERS: usize = 64;
const MAX_HEADER_SIZE: usize = 8192;
const MAX_BODY_SIZE: usize = 64 * 1024 * 1024;

/// Parses an HTTP/1.1 request from a byte buffer.
/// Returns Ok(Some(request)) when a full request is available,
/// Ok(None) when more data is needed, or Err on protocol violations.
pub fn parse_request(buf: &BytesMut) -> Result<Option<Request>> {
    // Find the end of headers
    let header_end = find_header_end(buf).ok_or_else(|| {
        if buf.len() > MAX_REQUEST_LINE + MAX_HEADERS * MAX_HEADER_SIZE {
            NanowebError::Http("Request headers too large".into())
        } else {
            NanowebError::Parse("Incomplete headers".into())
        }
    })?;

    let header_bytes = &buf[..header_end];
    let header_str = str::from_utf8(header_bytes)
        .map_err(|_| NanowebError::Http("Invalid UTF-8 in headers".into()))?;

    let mut lines = header_str.split("\r\n");

    // Parse request line
    let request_line = lines
        .next()
        .ok_or_else(|| NanowebError::Http("Empty request".into()))?;

    let (method, path, query, version) = parse_request_line(request_line)?;

    // Parse headers
    let mut headers = Headers::new();
    let mut header_count = 0;
    for line in lines {
        if line.is_empty() {
            break;
        }
        header_count += 1;
        if header_count > MAX_HEADERS {
            return Err(NanowebError::Http("Too many headers".into()));
        }
        if let Some((name, value)) = parse_header_line(line) {
            headers.insert(name, value);
        }
    }

    // Calculate body start
    let body_start = header_end + 4; // skip \r\n\r\n

    // Determine body length
    let content_length = headers
        .get("content-length")
        .and_then(|v| v.trim().parse::<usize>().ok());

    let is_chunked = headers
        .get("transfer-encoding")
        .map(|v| v.to_lowercase().contains("chunked"))
        .unwrap_or(false);

    let body_available = buf.len() - body_start;

    if is_chunked {
        // For chunked, we need to find the end marker (0\r\n\r\n)
        let remaining = &buf[body_start..];
        if let Some(body_end) = find_chunked_end(remaining) {
            let body = decode_chunked(&remaining[..body_end]);
            let mut request = build_request(method, path, query, version, headers, body);
            request.request_id = uuid::Uuid::new_v4().to_string();
            Ok(Some(request))
        } else {
            Ok(None) // need more data
        }
    } else if let Some(cl) = content_length {
        if cl > MAX_BODY_SIZE {
            return Err(NanowebError::BodyTooLarge);
        }
        if body_available >= cl {
            let body = buf[body_start..body_start + cl].to_vec();
            let mut request = build_request(method, path, query, version, headers, body);
            request.request_id = uuid::Uuid::new_v4().to_string();
            Ok(Some(request))
        } else {
            Ok(None) // need more data
        }
    } else {
        // No body
        let mut request = build_request(method, path, query, version, headers, Vec::new());
        request.request_id = uuid::Uuid::new_v4().to_string();
        Ok(Some(request))
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_request_line(line: &str) -> Result<(Method, String, String, String)> {
    let mut parts = line.splitn(3, ' ');
    let method_str = parts
        .next()
        .ok_or_else(|| NanowebError::Http("Missing method".into()))?;
    let raw_target = parts
        .next()
        .ok_or_else(|| NanowebError::Http("Missing target".into()))?;
    let version = parts
        .next()
        .ok_or_else(|| NanowebError::Http("Missing version".into()))?;

    let method = Method::from_bytes(method_str.as_bytes())
        .ok_or_else(|| NanowebError::Http(format!("Unknown method: {method_str}")))?;

    if !version.starts_with("HTTP/1.") {
        return Err(NanowebError::Http(format!(
            "Unsupported version: {version}"
        )));
    }

    // Split path and query
    let (path, query) = if let Some(pos) = raw_target.find('?') {
        (
            raw_target[..pos].to_string(),
            raw_target[pos + 1..].to_string(),
        )
    } else {
        (raw_target.to_string(), String::new())
    };

    Ok((method, path, query, version.to_string()))
}

fn parse_header_line(line: &str) -> Option<(String, String)> {
    let pos = line.find(':')?;
    let name = line[..pos].trim().to_string();
    let value = line[pos + 1..].trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some((name, value))
}

pub fn find_chunked_end(data: &[u8]) -> Option<usize> {
    // Chunked encoding ends with "0\r\n\r\n" (possibly with trailers)
    let pattern = b"0\r\n\r\n";
    if let Some(pos) = data.windows(5).position(|w| w == pattern) {
        return Some(pos + 5);
    }
    // Also handle "0\r\n" followed by trailers then "\r\n"
    let pattern2 = b"0\r\n";
    let mut i = 0;
    while i + 4 <= data.len() {
        if &data[i..i + 4] == pattern2 {
            // Check for end after trailers
            let rest = &data[i + 4..];
            if let Some(end) = rest.windows(2).position(|w| w == b"\r\n") {
                return Some(i + 4 + end + 2);
            }
        }
        i += 1;
    }
    None
}

fn decode_chunked(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut pos = 0;

    while pos < data.len() {
        // Read chunk size line
        let line_end = match data[pos..].windows(2).position(|w| w == b"\r\n") {
            Some(e) => e,
            None => break,
        };
        let size_str = str::from_utf8(&data[pos..pos + line_end]).unwrap_or("0");
        let size = usize::from_str_radix(size_str.trim(), 16).unwrap_or(0);

        if size == 0 {
            break;
        }

        pos += line_end + 2;
        let chunk_end = pos + size;
        if chunk_end <= data.len() {
            result.extend_from_slice(&data[pos..chunk_end]);
        }
        pos = chunk_end + 2; // skip trailing \r\n
    }

    result
}

fn build_request(
    method: Method,
    path: String,
    query: String,
    version: String,
    headers: Headers,
    body: Vec<u8>,
) -> Request {
    Request {
        method,
        path,
        query,
        version,
        headers,
        body,
        params: std::collections::HashMap::new(),
        request_id: String::new(),
    }
}

/// Format an HTTP response into bytes for sending over the wire.
pub fn format_response(resp: &crate::http::types::Response) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"HTTP/1.1 ");
    out.extend_from_slice(resp.status.0.to_string().as_bytes());
    out.extend_from_slice(b" ");
    out.extend_from_slice(resp.status.reason_phrase().as_bytes());
    out.extend_from_slice(b"\r\n");

    // Ensure content-length is set
    if !resp.headers.contains("content-length") {
        out.extend_from_slice(b"content-length: ");
        out.extend_from_slice(resp.body.len().to_string().as_bytes());
        out.extend_from_slice(b"\r\n");
    }

    for (name, value) in resp.headers.iter() {
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }

    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&resp.body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_get() {
        let input = b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let buf = BytesMut::from(&input[..]);
        let req = parse_request(&buf).unwrap().unwrap();
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.path, "/hello");
        assert_eq!(req.query, "");
        assert_eq!(req.header("host"), Some("localhost"));
    }

    #[test]
    fn parse_post_with_body() {
        let body = r#"{"name":"test"}"#;
        let input = format!(
            "POST /api HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let buf = BytesMut::from(input.as_bytes());
        let req = parse_request(&buf).unwrap().unwrap();
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.body, body.as_bytes());
    }

    #[test]
    fn parse_query_string() {
        let input = b"GET /search?q=hello&page=1 HTTP/1.1\r\n\r\n";
        let buf = BytesMut::from(&input[..]);
        let req = parse_request(&buf).unwrap().unwrap();
        assert_eq!(req.query, "q=hello&page=1");
    }

    #[test]
    fn reject_invalid_method() {
        let input = b"INVALID / HTTP/1.1\r\n\r\n";
        let buf = BytesMut::from(&input[..]);
        let result = parse_request(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn reject_bad_version() {
        let input = b"GET / HTTP/2.0\r\n\r\n";
        let buf = BytesMut::from(&input[..]);
        let result = parse_request(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn format_response_basic() {
        let resp = crate::http::types::Response::text("hello");
        let out = format_response(&resp);
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.contains("hello"));
    }

    #[test]
    fn parse_chunked_body() {
        let input =
            b"POST /data HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let buf = BytesMut::from(&input[..]);
        let req = parse_request(&buf).unwrap().unwrap();
        assert_eq!(req.body, b"hello");
    }
}

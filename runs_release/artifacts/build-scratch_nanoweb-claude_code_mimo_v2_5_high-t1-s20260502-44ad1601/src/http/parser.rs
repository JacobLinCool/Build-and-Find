use crate::error::{NanowebError, Result};
use bytes::BytesMut;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
    Other(String),
}

impl Method {
    pub fn from_str(s: &str) -> Self {
        match s {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "PATCH" => Method::Patch,
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            other => Method::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
            Method::Other(s) => s,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedRequestLine {
    pub method: Method,
    pub path: String,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct RawHttpRequest {
    pub request_line: ParsedRequestLine,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyEncoding {
    Length(usize),
    Chunked,
    None,
}

/// Parse a single HTTP/1.1 request from the buffer.
/// Returns `None` if not enough data is available yet (need more bytes).
pub fn parse_request(buf: &BytesMut) -> Result<Option<RawHttpRequest>> {
    let data = buf.as_ref();
    // Find the end of headers (double CRLF)
    let header_end = find_header_end(data)?;

    // Not found yet - need more data
    if header_end == usize::MAX {
        return Ok(None);
    }

    let header_bytes = &data[..header_end];
    let header_str = std::str::from_utf8(header_bytes)
        .map_err(|e| NanowebError::HttpParse(format!("invalid UTF-8 in headers: {e}")))?;

    let mut lines = header_str.split("\r\n");

    // Parse request line
    let request_line_str = lines
        .next()
        .ok_or_else(|| NanowebError::HttpParse("empty request".into()))?;
    let request_line = parse_request_line(request_line_str)?;

    // Parse headers
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.insert(key.trim().to_lowercase(), value.trim().to_string());
        }
    }

    // Determine body encoding
    let body_start = header_end + 4; // skip \r\n\r\n
    let available = data.len() - body_start;

    let body_encoding = if let Some(cl) = headers.get("content-length") {
        let len: usize = cl
            .parse()
            .map_err(|_| NanowebError::HttpParse("invalid Content-Length".into()))?;
        BodyEncoding::Length(len)
    } else if headers.get("transfer-encoding").map(|s| s.as_str()) == Some("chunked") {
        BodyEncoding::Chunked
    } else {
        BodyEncoding::None
    };

    match body_encoding {
        BodyEncoding::None => Ok(Some(RawHttpRequest {
            request_line,
            headers,
            body: Vec::new(),
        })),
        BodyEncoding::Length(len) => {
            if available < len {
                return Ok(None); // need more data
            }
            let body = data[body_start..body_start + len].to_vec();
            Ok(Some(RawHttpRequest {
                request_line,
                headers,
                body,
            }))
        }
        BodyEncoding::Chunked => parse_chunked_body(data, body_start, &request_line, headers),
    }
}

fn find_header_end(data: &[u8]) -> Result<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if &data[i..i + 4] == b"\r\n\r\n" {
            return Ok(i);
        }
    }
    // Not found yet - return max to signal incomplete
    Ok(usize::MAX)
}

fn parse_request_line(line: &str) -> Result<ParsedRequestLine> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(NanowebError::HttpParse(format!(
            "invalid request line: {line}"
        )));
    }

    let method = Method::from_str(parts[0]);
    let path = parts[1].to_string();
    let version = parts[2].to_string();

    if !version.starts_with("HTTP/") {
        return Err(NanowebError::HttpParse(format!(
            "invalid HTTP version: {version}"
        )));
    }

    Ok(ParsedRequestLine {
        method,
        path,
        version,
    })
}

fn parse_chunked_body(
    data: &[u8],
    start: usize,
    request_line: &ParsedRequestLine,
    headers: HashMap<String, String>,
) -> Result<Option<RawHttpRequest>> {
    let mut pos = start;
    let mut body = Vec::new();

    loop {
        // Read chunk size line
        if let Some(line_end) = find_line_end(data, pos) {
            let size_str = std::str::from_utf8(&data[pos..line_end])
                .map_err(|e| NanowebError::HttpParse(format!("invalid chunk size: {e}")))?;
            let size = usize::from_str_radix(size_str.trim(), 16).map_err(|_| {
                NanowebError::HttpParse(format!("invalid chunk size hex: {size_str}"))
            })?;

            pos = line_end + 2; // skip \r\n

            if size == 0 {
                // Final chunk
                return Ok(Some(RawHttpRequest {
                    request_line: request_line.clone(),
                    headers,
                    body,
                }));
            }

            if data.len() < pos + size + 2 {
                return Ok(None); // need more data
            }

            body.extend_from_slice(&data[pos..pos + size]);
            pos += size + 2; // skip chunk data + \r\n
        } else {
            return Ok(None); // need more data
        }
    }
}

fn find_line_end(data: &[u8], start: usize) -> Option<usize> {
    for i in start..data.len().saturating_sub(1) {
        if data[i] == b'\r' && data[i + 1] == b'\n' {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_get() {
        let req = b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let buf = BytesMut::from(&req[..]);
        let parsed = parse_request(&buf).unwrap().unwrap();
        assert_eq!(parsed.request_line.method, Method::Get);
        assert_eq!(parsed.request_line.path, "/hello");
        assert_eq!(parsed.headers.get("host").unwrap(), "localhost");
        assert!(parsed.body.is_empty());
    }

    #[test]
    fn test_parse_post_with_body() {
        let req = b"POST /data HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n{\"key\":\"val\"}";
        let buf = BytesMut::from(&req[..]);
        let parsed = parse_request(&buf).unwrap().unwrap();
        assert_eq!(parsed.request_line.method, Method::Post);
        assert_eq!(parsed.body, b"{\"key\":\"val\"}");
    }

    #[test]
    fn test_parse_chunked() {
        let req = b"POST /data HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let buf = BytesMut::from(&req[..]);
        let parsed = parse_request(&buf).unwrap().unwrap();
        assert_eq!(parsed.body, b"hello world");
    }

    #[test]
    fn test_invalid_method() {
        let req = b"INVALID / HTTP/1.1\r\n\r\n";
        let buf = BytesMut::from(&req[..]);
        let parsed = parse_request(&buf).unwrap().unwrap();
        assert_eq!(parsed.request_line.method, Method::Other("INVALID".into()));
    }

    #[test]
    fn test_incomplete_request() {
        let req = b"GET /hello HTTP/1.1\r\nHost: ";
        let buf = BytesMut::from(&req[..]);
        assert!(parse_request(&buf).unwrap().is_none());
    }

    #[test]
    fn test_400_invalid_request_line() {
        let req = b"BADLINE\r\n\r\n";
        let buf = BytesMut::from(&req[..]);
        assert!(parse_request(&buf).is_err());
    }
}

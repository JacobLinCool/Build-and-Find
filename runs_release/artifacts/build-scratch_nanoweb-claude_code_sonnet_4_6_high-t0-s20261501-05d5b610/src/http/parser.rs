//! HTTP/1.1 byte-level parser. Has no dependencies on router, middleware, or body modules.

use crate::error::NanowebError;
use crate::http::types::{Headers, Method, Version};

const MAX_HEADER_SIZE: usize = 16 * 1024; // 16 KB
const MAX_BODY_SIZE: usize = 64 * 1024 * 1024; // 64 MB

#[derive(Debug)]
pub struct ParsedRequest {
    pub method: Method,
    pub target: String,
    pub version: Version,
    pub headers: Headers,
    pub body: Vec<u8>,
}

/// Parses a complete HTTP/1.1 request from a byte slice.
/// Returns the parsed request and the number of bytes consumed.
pub fn parse_request(buf: &[u8]) -> Result<Option<(ParsedRequest, usize)>, NanowebError> {
    // Find the end of headers (CRLFCRLF)
    let header_end = find_double_crlf(buf);
    let header_end = match header_end {
        Some(pos) => pos,
        None => {
            if buf.len() > MAX_HEADER_SIZE {
                return Err(NanowebError::BadRequest("Headers too large".into()));
            }
            return Ok(None); // Need more data
        }
    };

    let header_section = &buf[..header_end];
    if header_section.len() > MAX_HEADER_SIZE {
        return Err(NanowebError::BadRequest("Headers too large".into()));
    }

    // Parse request line + headers
    let (method, target, version, headers) = parse_headers(header_section)?;

    // Determine body length
    let headers_total = header_end + 4; // +4 for the CRLFCRLF
    let body_start = headers_total;
    let remaining = &buf[body_start..];

    let (body, total_consumed) = if let Some(te) = headers.get("transfer-encoding") {
        if te.to_lowercase().contains("chunked") {
            match parse_chunked_body(remaining)? {
                Some((body, consumed)) => (body, body_start + consumed),
                None => return Ok(None), // Need more data
            }
        } else {
            return Err(NanowebError::BadRequest(format!(
                "Unsupported transfer-encoding: {}",
                te
            )));
        }
    } else if let Some(cl) = headers.get("content-length") {
        let content_length: usize = cl
            .trim()
            .parse()
            .map_err(|_| NanowebError::BadRequest("Invalid Content-Length".into()))?;
        if content_length > MAX_BODY_SIZE {
            return Err(NanowebError::BodyTooLarge);
        }
        if remaining.len() < content_length {
            return Ok(None); // Need more data
        }
        let body = remaining[..content_length].to_vec();
        (body, body_start + content_length)
    } else {
        // No body
        (Vec::new(), body_start)
    };

    Ok(Some((
        ParsedRequest {
            method,
            target,
            version,
            headers,
            body,
        },
        total_consumed,
    )))
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_headers(data: &[u8]) -> Result<(Method, String, Version, Headers), NanowebError> {
    let text = std::str::from_utf8(data)
        .map_err(|_| NanowebError::BadRequest("Non-UTF8 in headers".into()))?;

    let mut lines = text.split("\r\n");

    // Parse request line
    let request_line = lines
        .next()
        .ok_or_else(|| NanowebError::BadRequest("Empty request".into()))?;

    let (method, target, version) = parse_request_line(request_line)?;

    // Parse header fields
    let mut headers = Headers::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = parse_header_line(line)?;
        headers.append(name, value);
    }

    Ok((method, target, version, headers))
}

fn parse_request_line(line: &str) -> Result<(Method, String, Version), NanowebError> {
    let mut parts = line.splitn(3, ' ');

    let method_str = parts
        .next()
        .ok_or_else(|| NanowebError::BadRequest("Missing method".into()))?;
    let target = parts
        .next()
        .ok_or_else(|| NanowebError::BadRequest("Missing request target".into()))?
        .to_string();
    let version_str = parts
        .next()
        .ok_or_else(|| NanowebError::BadRequest("Missing HTTP version".into()))?
        .trim_end_matches('\r');

    if method_str.is_empty() {
        return Err(NanowebError::BadRequest("Empty method".into()));
    }
    if target.is_empty() {
        return Err(NanowebError::BadRequest("Empty target".into()));
    }

    // Validate method token characters (RFC 7230)
    if !method_str.bytes().all(|b| is_tchar(b)) {
        return Err(NanowebError::BadRequest(format!(
            "Invalid method: {}",
            method_str
        )));
    }

    let version = Version::from_str(version_str).ok_or_else(|| {
        NanowebError::BadRequest(format!("Unsupported HTTP version: {}", version_str))
    })?;

    Ok((Method::from_str(method_str), target, version))
}

fn parse_header_line(line: &str) -> Result<(String, String), NanowebError> {
    let colon = line
        .find(':')
        .ok_or_else(|| NanowebError::BadRequest(format!("Invalid header line: {}", line)))?;

    let name = line[..colon].trim().to_string();
    let value = line[colon + 1..].trim().to_string();

    // Validate header name (token characters only)
    if name.is_empty() || !name.bytes().all(|b| is_tchar(b)) {
        return Err(NanowebError::BadRequest(format!(
            "Invalid header name: {}",
            name
        )));
    }

    // Reject obs-fold (multi-line headers)
    if value.contains('\n') || value.contains('\r') {
        return Err(NanowebError::BadRequest("Folded header value".into()));
    }

    Ok((name, value))
}

fn parse_chunked_body(data: &[u8]) -> Result<Option<(Vec<u8>, usize)>, NanowebError> {
    let mut pos = 0;
    let mut body = Vec::new();

    loop {
        // Find end of chunk size line
        let crlf = find_crlf(&data[pos..]);
        let crlf = match crlf {
            Some(p) => p,
            None => return Ok(None),
        };

        let size_line = std::str::from_utf8(&data[pos..pos + crlf])
            .map_err(|_| NanowebError::BadRequest("Invalid chunk size".into()))?;

        // Handle chunk extensions (ignore them)
        let size_str = size_line.split(';').next().unwrap_or("").trim();
        let chunk_size = usize::from_str_radix(size_str, 16)
            .map_err(|_| NanowebError::BadRequest(format!("Invalid chunk size: {}", size_str)))?;

        pos += crlf + 2; // Skip the size line + CRLF

        if chunk_size == 0 {
            // Last chunk - skip trailing CRLF
            if data.len() < pos + 2 {
                return Ok(None); // Need more data for final CRLF
            }
            pos += 2; // Skip final CRLF
            return Ok(Some((body, pos)));
        }

        if body.len() + chunk_size > MAX_BODY_SIZE {
            return Err(NanowebError::BodyTooLarge);
        }

        // Check we have enough data
        if data.len() < pos + chunk_size + 2 {
            return Ok(None); // Need more data
        }

        body.extend_from_slice(&data[pos..pos + chunk_size]);
        pos += chunk_size + 2; // Skip chunk data + CRLF
    }
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

fn is_tchar(b: u8) -> bool {
    matches!(b,
        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' |
        b'^' | b'_' | b'`' | b'|' | b'~' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_get() {
        let raw = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let result = parse_request(raw).unwrap().unwrap();
        let (req, consumed) = result;
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.target, "/");
        assert_eq!(req.version, Version::Http11);
        assert_eq!(req.headers.get("host"), Some("example.com"));
        assert!(req.body.is_empty());
        assert_eq!(consumed, raw.len());
    }

    #[test]
    fn test_parse_post_with_body() {
        let raw = b"POST /submit HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let (req, consumed) = parse_request(raw).unwrap().unwrap();
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.body, b"hello");
        assert_eq!(consumed, raw.len());
    }

    #[test]
    fn test_parse_chunked() {
        let raw = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let (req, _) = parse_request(raw).unwrap().unwrap();
        assert_eq!(req.body, b"hello");
    }

    #[test]
    fn test_incomplete_returns_none() {
        let raw = b"GET / HTTP/1.1\r\n";
        let result = parse_request(raw).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_invalid_method_rejected() {
        let raw = b"G@T / HTTP/1.1\r\n\r\n";
        let result = parse_request(raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_version_rejected() {
        let raw = b"GET / HTTP/2.0\r\n\r\n";
        let result = parse_request(raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_path_with_query() {
        let raw = b"GET /search?q=hello&page=1 HTTP/1.1\r\n\r\n";
        let (req, _) = parse_request(raw).unwrap().unwrap();
        assert_eq!(req.target, "/search?q=hello&page=1");
    }

    #[test]
    fn test_multiple_headers() {
        let raw = b"GET / HTTP/1.1\r\nHost: foo\r\nAccept: text/html\r\nX-Custom: value\r\n\r\n";
        let (req, _) = parse_request(raw).unwrap().unwrap();
        assert_eq!(req.headers.get("host"), Some("foo"));
        assert_eq!(req.headers.get("accept"), Some("text/html"));
        assert_eq!(req.headers.get("x-custom"), Some("value"));
    }

    #[test]
    fn test_content_length_partial() {
        let raw = b"POST / HTTP/1.1\r\nContent-Length: 10\r\n\r\npartial";
        let result = parse_request(raw).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_chunked_multiple_chunks() {
        let raw = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nfoo\r\n3\r\nbar\r\n0\r\n\r\n";
        let (req, _) = parse_request(raw).unwrap().unwrap();
        assert_eq!(req.body, b"foobar");
    }
}

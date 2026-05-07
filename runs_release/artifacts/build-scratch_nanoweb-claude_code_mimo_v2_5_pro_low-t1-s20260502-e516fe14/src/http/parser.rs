use super::types::*;
use crate::error::NanowebError;
use std::collections::HashMap;

/// Maximum allowed header size (8KB).
const MAX_HEADER_SIZE: usize = 8192;
/// Maximum allowed body size (16MB default).
pub const MAX_BODY_SIZE: usize = 16 * 1024 * 1024;

/// Parse a raw HTTP/1.1 request from bytes.
pub fn parse_request(raw: &[u8]) -> Result<Request, NanowebError> {
    // Find the end of headers
    let header_end = find_header_end(raw)
        .ok_or_else(|| NanowebError::HttpParse("Could not find end of headers".into()))?;

    let header_bytes = &raw[..header_end];
    if header_bytes.len() > MAX_HEADER_SIZE {
        return Err(NanowebError::HttpParse(
            "Headers exceed maximum size".into(),
        ));
    }

    let header_str = std::str::from_utf8(header_bytes)
        .map_err(|_| NanowebError::HttpParse("Invalid UTF-8 in headers".into()))?;

    let mut lines = header_str.split("\r\n");

    // Parse request line
    let request_line = lines
        .next()
        .ok_or_else(|| NanowebError::HttpParse("Empty request".into()))?;
    let (method, target, version) = parse_request_line(request_line)?;

    // Parse headers
    let mut headers = HeaderMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let colon_pos = line
            .find(':')
            .ok_or_else(|| NanowebError::HttpParse("Invalid header line".into()))?;
        let name = line[..colon_pos].trim().to_string();
        let value = line[colon_pos + 1..].trim().to_string();
        headers.append(name, value);
    }

    // Parse body based on transfer encoding and content-length
    let body_start = header_end + 4; // Skip \r\n\r\n
    let body = if headers
        .get("transfer-encoding")
        .map(|v| v.to_lowercase().contains("chunked"))
        .unwrap_or(false)
    {
        parse_chunked_body(&raw[body_start..])?
    } else if let Some(content_length) = headers.get("content-length") {
        let len: usize = content_length
            .parse()
            .map_err(|_| NanowebError::HttpParse("Invalid Content-Length".into()))?;
        if len > MAX_BODY_SIZE {
            return Err(NanowebError::HttpParse("Body exceeds maximum size".into()));
        }
        if body_start + len > raw.len() {
            return Err(NanowebError::HttpParse("Incomplete body".into()));
        }
        raw[body_start..body_start + len].to_vec()
    } else {
        Vec::new()
    };

    // Parse path and query from target
    let (path, query) = parse_target(&target);

    Ok(Request {
        method,
        target,
        path,
        version,
        headers,
        body,
        query,
        params: HashMap::new(),
        request_id: String::new(),
        session_data: HashMap::new(),
    })
}

/// Find the position of \r\n\r\n in the buffer.
fn find_header_end(buf: &[u8]) -> Option<usize> {
    for i in 0..buf.len().saturating_sub(3) {
        if &buf[i..i + 4] == b"\r\n\r\n" {
            return Some(i);
        }
    }
    None
}

/// Parse the HTTP request line: METHOD TARGET VERSION
fn parse_request_line(line: &str) -> Result<(Method, String, String), NanowebError> {
    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    if parts.len() != 3 {
        return Err(NanowebError::HttpParse("Invalid request line".into()));
    }

    let method = Method::from_str(parts[0])
        .ok_or_else(|| NanowebError::HttpParse(format!("Unknown method: {}", parts[0])))?;

    let version = parts[2].to_string();
    if !version.starts_with("HTTP/1.") {
        return Err(NanowebError::HttpParse(format!(
            "Unsupported HTTP version: {}",
            version
        )));
    }

    Ok((method, parts[1].to_string(), version))
}

/// Parse a chunked transfer-encoded body.
fn parse_chunked_body(buf: &[u8]) -> Result<Vec<u8>, NanowebError> {
    let mut result = Vec::new();
    let mut pos = 0;

    loop {
        // Find the chunk size line
        let line_end = buf[pos..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| NanowebError::HttpParse("Invalid chunked encoding".into()))?;

        let size_str = std::str::from_utf8(&buf[pos..pos + line_end])
            .unwrap_or("")
            .trim();

        // Handle chunk extensions (ignore them)
        let size_str = size_str.split(';').next().unwrap_or("").trim();

        let chunk_size = u64::from_str_radix(size_str, 16)
            .map_err(|_| NanowebError::HttpParse("Invalid chunk size".into()))?;

        if chunk_size == 0 {
            // Terminal chunk
            break;
        }

        pos += line_end + 2; // Skip \r\n after size

        if pos + chunk_size as usize > buf.len() {
            return Err(NanowebError::HttpParse("Incomplete chunked body".into()));
        }

        result.extend_from_slice(&buf[pos..pos + chunk_size as usize]);
        pos += chunk_size as usize;

        // Skip trailing \r\n
        if pos + 2 > buf.len() {
            return Err(NanowebError::HttpParse("Invalid chunked encoding".into()));
        }
        pos += 2;
    }

    Ok(result)
}

/// Parse the request target into path and query string.
fn parse_target(target: &str) -> (String, HashMap<String, String>) {
    if let Some(qpos) = target.find('?') {
        let path = url_decode(&target[..qpos]);
        let query = parse_query_string(&target[qpos + 1..]);
        (path, query)
    } else {
        (url_decode(target), HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_get() {
        let raw = b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = parse_request(raw).unwrap();
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.path, "/hello");
        assert_eq!(req.version, "HTTP/1.1");
        assert_eq!(req.header("host"), Some("localhost"));
        assert!(req.body.is_empty());
    }

    #[test]
    fn test_parse_post_with_body() {
        let raw = b"POST /data HTTP/1.1\r\nContent-Length: 13\r\nContent-Type: application/json\r\n\r\n{\"key\":\"val\"}";
        let req = parse_request(raw).unwrap();
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.body, b"{\"key\":\"val\"}");
    }

    #[test]
    fn test_parse_chunked_body() {
        let raw = b"POST /data HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let req = parse_request(raw).unwrap();
        assert_eq!(req.body, b"hello world");
    }

    #[test]
    fn test_parse_query_string() {
        let raw = b"GET /search?q=hello+world&lang=en HTTP/1.1\r\n\r\n";
        let req = parse_request(raw).unwrap();
        assert_eq!(req.path, "/search");
        assert_eq!(req.query.get("q").unwrap(), "hello world");
        assert_eq!(req.query.get("lang").unwrap(), "en");
    }

    #[test]
    fn test_reject_invalid_method() {
        let raw = b"INVALID /path HTTP/1.1\r\n\r\n";
        assert!(parse_request(raw).is_err());
    }

    #[test]
    fn test_reject_non_http1() {
        let raw = b"GET /path HTTP/2.0\r\n\r\n";
        assert!(parse_request(raw).is_err());
    }

    #[test]
    fn test_parse_multiple_headers() {
        let raw =
            b"GET / HTTP/1.1\r\nHost: localhost\r\nAccept: text/html\r\nX-Custom: value\r\n\r\n";
        let req = parse_request(raw).unwrap();
        assert_eq!(req.header("host"), Some("localhost"));
        assert_eq!(req.header("accept"), Some("text/html"));
        assert_eq!(req.header("x-custom"), Some("value"));
    }

    #[test]
    fn test_parse_request_with_query_params() {
        let raw = b"GET /api/users?page=1&limit=10 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = parse_request(raw).unwrap();
        assert_eq!(req.path, "/api/users");
        assert_eq!(req.query.get("page").unwrap(), "1");
        assert_eq!(req.query.get("limit").unwrap(), "10");
    }

    #[test]
    fn test_empty_request() {
        assert!(parse_request(b"").is_err());
    }

    #[test]
    fn test_chunked_with_extensions() {
        let raw = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n7;ext=foo\r\nMozilla\r\n9\r\nDeveloper\r\n0\r\n\r\n";
        let req = parse_request(raw).unwrap();
        assert_eq!(req.body, b"MozillaDeveloper");
    }
}

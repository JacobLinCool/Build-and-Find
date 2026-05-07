//! Byte-level HTTP/1.1 request parser.
//!
//! This module is intentionally self-contained: it depends only on the request
//! and header types and on the crate-wide error type. It does not know about
//! routers, middleware, or body parsers and can be exercised in isolation.

use super::types::{Headers, Method, Request, Version};
use crate::error::{NanowebError, Result};

/// Maximum allowed size of the head (request line + headers) in bytes.
pub const MAX_HEAD_SIZE: usize = 64 * 1024;
/// Maximum allowed body size in bytes (16 MiB).
pub const MAX_BODY_SIZE: usize = 16 * 1024 * 1024;

/// Parse the head of an HTTP/1.1 request from a byte slice.
///
/// `buf` must contain at least the start-line and the header block terminated
/// by a CRLFCRLF sequence. The function returns the parsed [`Request`] (with
/// an empty body) and the number of bytes consumed from `buf`.
pub fn parse_request_head(buf: &[u8]) -> Result<(Request, usize)> {
    let head_end =
        find_double_crlf(buf).ok_or_else(|| NanowebError::Parse("incomplete head".into()))?;
    if head_end > MAX_HEAD_SIZE {
        return Err(NanowebError::Parse("head too large".into()));
    }
    let head = &buf[..head_end];
    let mut lines = split_crlf(head);
    let start_line = lines
        .next()
        .ok_or_else(|| NanowebError::Parse("empty start-line".into()))?;
    let (method, target, version) = parse_request_line(start_line)?;

    let mut headers = Headers::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = parse_header_line(line)?;
        headers.append(name, value);
    }

    let (path, query) = match target.find('?') {
        Some(i) => (target[..i].to_string(), target[i + 1..].to_string()),
        None => (target.to_string(), String::new()),
    };

    let mut req = Request::new();
    req.method = method;
    req.target = target.to_string();
    req.path = path;
    req.query = query;
    req.version = version;
    req.headers = headers;
    Ok((req, head_end + 4))
}

fn parse_request_line(line: &str) -> Result<(Method, &str, Version)> {
    let mut parts = line.splitn(3, ' ');
    let method_s = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing method".into()))?;
    let target = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing target".into()))?;
    let version_s = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing version".into()))?;
    if !is_valid_token(method_s.as_bytes()) {
        return Err(NanowebError::Parse("invalid method".into()));
    }
    if target.is_empty() || target.contains(|c: char| c.is_ascii_control()) {
        return Err(NanowebError::Parse("invalid target".into()));
    }
    let version = Version::parse(version_s)
        .ok_or_else(|| NanowebError::Parse(format!("unsupported http version: {}", version_s)))?;
    Ok((Method::parse(method_s), target, version))
}

fn parse_header_line(line: &str) -> Result<(String, String)> {
    let colon = line
        .find(':')
        .ok_or_else(|| NanowebError::Parse("malformed header".into()))?;
    let name = line[..colon].to_string();
    if name.is_empty() || !is_valid_token(name.as_bytes()) {
        return Err(NanowebError::Parse("invalid header name".into()));
    }
    let value = line[colon + 1..].trim().to_string();
    Ok((name, value))
}

fn is_valid_token(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    bytes.iter().all(|&b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'!' | b'#'
                    | b'$'
                    | b'%'
                    | b'&'
                    | b'\''
                    | b'*'
                    | b'+'
                    | b'-'
                    | b'.'
                    | b'^'
                    | b'_'
                    | b'`'
                    | b'|'
                    | b'~'
            )
    })
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    for i in 0..buf.len() - 3 {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' && buf[i + 2] == b'\r' && buf[i + 3] == b'\n' {
            return Some(i);
        }
    }
    None
}

fn split_crlf(buf: &[u8]) -> impl Iterator<Item = &str> {
    let s = std::str::from_utf8(buf).unwrap_or("");
    s.split("\r\n").filter(|line| !line.is_empty())
}

/// Body reading mode determined from request headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyMode {
    /// No body expected.
    None,
    /// Body terminated by Content-Length.
    Length(usize),
    /// Body uses chunked transfer encoding.
    Chunked,
}

pub fn body_mode(req: &Request) -> Result<BodyMode> {
    let te = req
        .headers
        .get("transfer-encoding")
        .map(str::to_ascii_lowercase);
    let cl = req.headers.get("content-length");
    if let Some(te) = te {
        if te
            .split(',')
            .map(|s| s.trim())
            .any(|s| s.eq_ignore_ascii_case("chunked"))
        {
            return Ok(BodyMode::Chunked);
        }
        if !te.trim().is_empty() && te.trim() != "identity" {
            return Err(NanowebError::Parse(format!(
                "unsupported transfer-encoding: {}",
                te
            )));
        }
    }
    if let Some(cl) = cl {
        let n: usize = cl
            .trim()
            .parse()
            .map_err(|_| NanowebError::Parse("invalid content-length".into()))?;
        if n > MAX_BODY_SIZE {
            return Err(NanowebError::Parse("body too large".into()));
        }
        return Ok(BodyMode::Length(n));
    }
    Ok(BodyMode::None)
}

/// Parse a chunked-transfer-encoded body from a complete byte slice.
pub fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let crlf =
            find_crlf(data).ok_or_else(|| NanowebError::Parse("malformed chunk header".into()))?;
        let line = std::str::from_utf8(&data[..crlf])
            .map_err(|_| NanowebError::Parse("invalid chunk header utf-8".into()))?;
        // Strip optional chunk extensions (after ';').
        let size_str = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_str, 16)
            .map_err(|_| NanowebError::Parse("invalid chunk size".into()))?;
        data = &data[crlf + 2..];
        if size == 0 {
            // Optional trailers, then a final CRLF.
            // Skip until a blank line (CRLFCRLF or just CRLF if no trailers).
            // Per RFC 7230, the trailer section ends with CRLF.
            // Accept any number of trailer lines.
            loop {
                let line_end = find_crlf(data)
                    .ok_or_else(|| NanowebError::Parse("malformed trailer".into()))?;
                let t = &data[..line_end];
                data = &data[line_end + 2..];
                if t.is_empty() {
                    break;
                }
            }
            break;
        }
        if data.len() < size + 2 {
            return Err(NanowebError::Parse("chunk truncated".into()));
        }
        out.extend_from_slice(&data[..size]);
        if &data[size..size + 2] != b"\r\n" {
            return Err(NanowebError::Parse("missing chunk crlf".into()));
        }
        data = &data[size + 2..];
        if out.len() > MAX_BODY_SIZE {
            return Err(NanowebError::Parse("body too large".into()));
        }
    }
    Ok(out)
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    if buf.len() < 2 {
        return None;
    }
    for i in 0..buf.len() - 1 {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_get() {
        let raw = b"GET /hello?x=1 HTTP/1.1\r\nHost: example.com\r\nUser-Agent: t\r\n\r\n";
        let (req, used) = parse_request_head(raw).unwrap();
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.path, "/hello");
        assert_eq!(req.query, "x=1");
        assert_eq!(req.version, Version::Http11);
        assert_eq!(req.headers.get("host"), Some("example.com"));
        assert_eq!(used, raw.len());
    }

    #[test]
    fn rejects_bad_version() {
        let raw = b"GET / HTTP/9.9\r\n\r\n";
        assert!(parse_request_head(raw).is_err());
    }

    #[test]
    fn rejects_invalid_header() {
        let raw = b"GET / HTTP/1.1\r\nbad header\r\n\r\n";
        assert!(parse_request_head(raw).is_err());
    }

    #[test]
    fn detects_chunked() {
        let raw = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        let (req, _) = parse_request_head(raw).unwrap();
        assert_eq!(body_mode(&req).unwrap(), BodyMode::Chunked);
    }

    #[test]
    fn detects_content_length() {
        let raw = b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\n";
        let (req, _) = parse_request_head(raw).unwrap();
        assert_eq!(body_mode(&req).unwrap(), BodyMode::Length(5));
    }

    #[test]
    fn decodes_chunked_body() {
        let body = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let decoded = decode_chunked(body).unwrap();
        assert_eq!(decoded, b"hello world");
    }

    #[test]
    fn rejects_negative_content_length() {
        let raw = b"POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n";
        let (req, _) = parse_request_head(raw).unwrap();
        assert!(body_mode(&req).is_err());
    }
}

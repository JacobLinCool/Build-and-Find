use crate::http::types::{Method, Request};
use std::collections::HashMap;

const MAX_HEADERS: usize = 100;
const MAX_HEADER_SIZE: usize = 8192;
const MAX_REQUEST_LINE: usize = 8192;

#[derive(Debug)]
pub enum ParseError {
    Incomplete,
    InvalidRequestLine(String),
    InvalidHeader(String),
    InvalidBody(String),
    TooLarge,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Incomplete => write!(f, "Incomplete request"),
            ParseError::InvalidRequestLine(s) => write!(f, "Invalid request line: {}", s),
            ParseError::InvalidHeader(s) => write!(f, "Invalid header: {}", s),
            ParseError::InvalidBody(s) => write!(f, "Invalid body: {}", s),
            ParseError::TooLarge => write!(f, "Request too large"),
        }
    }
}

pub struct Parser;

impl Parser {
    /// Parse a complete HTTP/1.1 request from bytes.
    /// Returns (Request, bytes_consumed) on success.
    pub fn parse(buf: &[u8]) -> Result<(Request, usize), ParseError> {
        if buf.len() > MAX_HEADER_SIZE * 10 {
            return Err(ParseError::TooLarge);
        }

        // Find end of headers (\r\n\r\n)
        let header_end = find_double_crlf(buf).ok_or(ParseError::Incomplete)?;
        let header_section = &buf[..header_end];

        if header_section.len() > MAX_REQUEST_LINE + MAX_HEADERS * 256 {
            return Err(ParseError::TooLarge);
        }

        // Split into lines
        let mut lines = split_crlf(header_section);
        if lines.is_empty() {
            return Err(ParseError::InvalidRequestLine("empty request".to_string()));
        }

        // Parse request line
        let request_line = lines.remove(0);
        let (method, path, query, version) = parse_request_line(request_line)?;

        // Parse headers
        let mut headers = HashMap::new();
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let (name, value) = parse_header(line)?;
            headers.insert(name.to_lowercase(), value);
        }

        let body_start = header_end + 4; // skip \r\n\r\n

        // Determine body length
        let body = if let Some(cl) = headers.get("content-length") {
            let len: usize = cl
                .trim()
                .parse()
                .map_err(|_| ParseError::InvalidHeader("invalid content-length".to_string()))?;
            if body_start + len > buf.len() {
                return Err(ParseError::Incomplete);
            }
            buf[body_start..body_start + len].to_vec()
        } else if headers
            .get("transfer-encoding")
            .map(|v| v.to_lowercase().contains("chunked"))
            .unwrap_or(false)
        {
            parse_chunked_body(&buf[body_start..])?
        } else {
            Vec::new()
        };

        let body_len = if headers
            .get("transfer-encoding")
            .map(|v| v.to_lowercase().contains("chunked"))
            .unwrap_or(false)
        {
            // For chunked, find the end
            let consumed = find_chunked_end(&buf[body_start..]).unwrap_or(buf.len() - body_start);
            consumed
        } else {
            body.len()
        };

        let total_consumed = body_start + body_len;

        let mut req = Request::new(method, path);
        req.query = query;
        req.version = version;
        req.headers = headers;
        req.body = body;

        Ok((req, total_consumed))
    }
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    let needle = b"\r\n\r\n";
    buf.windows(4).position(|w| w == needle)
}

fn split_crlf(buf: &[u8]) -> Vec<&[u8]> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i + 1 < buf.len() {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            result.push(&buf[start..i]);
            start = i + 2;
            i = start;
        } else {
            i += 1;
        }
    }
    if start < buf.len() {
        result.push(&buf[start..]);
    }
    result
}

fn parse_request_line(line: &[u8]) -> Result<(Method, String, String, String), ParseError> {
    let s = std::str::from_utf8(line)
        .map_err(|_| ParseError::InvalidRequestLine("invalid UTF-8".to_string()))?;

    let parts: Vec<&str> = s.splitn(3, ' ').collect();
    if parts.len() != 3 {
        return Err(ParseError::InvalidRequestLine(format!(
            "expected 3 parts, got {}",
            parts.len()
        )));
    }

    let method = Method::from_str(parts[0]);

    let request_target = parts[1];
    let (path, query) = if let Some(pos) = request_target.find('?') {
        (
            request_target[..pos].to_string(),
            request_target[pos + 1..].to_string(),
        )
    } else {
        (request_target.to_string(), String::new())
    };

    let version = parts[2].trim().to_string();
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(ParseError::InvalidRequestLine(format!(
            "unsupported version: {}",
            version
        )));
    }

    Ok((method, path, query, version))
}

fn parse_header(line: &[u8]) -> Result<(String, String), ParseError> {
    let s = std::str::from_utf8(line)
        .map_err(|_| ParseError::InvalidHeader("invalid UTF-8 in header".to_string()))?;

    let colon_pos = s
        .find(':')
        .ok_or_else(|| ParseError::InvalidHeader(format!("no colon in header: {}", s)))?;

    let name = s[..colon_pos].trim().to_string();
    let value = s[colon_pos + 1..].trim().to_string();

    if name.is_empty() {
        return Err(ParseError::InvalidHeader("empty header name".to_string()));
    }

    Ok((name, value))
}

fn parse_chunked_body(buf: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut body = Vec::new();
    let mut pos = 0;

    loop {
        // Read chunk size line
        let line_end = find_crlf(&buf[pos..]).ok_or(ParseError::Incomplete)?;
        let size_str = std::str::from_utf8(&buf[pos..pos + line_end])
            .map_err(|_| ParseError::InvalidBody("invalid chunk size".to_string()))?;
        // Chunk size may have extensions after semicolon
        let size_str = size_str.split(';').next().unwrap_or("").trim();
        let chunk_size = usize::from_str_radix(size_str, 16).map_err(|_| {
            ParseError::InvalidBody(format!("invalid chunk size hex: {}", size_str))
        })?;

        pos += line_end + 2; // skip \r\n

        if chunk_size == 0 {
            // Last chunk, skip trailing \r\n
            break;
        }

        if pos + chunk_size > buf.len() {
            return Err(ParseError::Incomplete);
        }

        body.extend_from_slice(&buf[pos..pos + chunk_size]);
        pos += chunk_size + 2; // skip chunk data + \r\n
    }

    Ok(body)
}

fn find_chunked_end(buf: &[u8]) -> Option<usize> {
    let mut pos = 0;
    loop {
        let line_end = find_crlf(&buf[pos..])?;
        let size_str = std::str::from_utf8(&buf[pos..pos + line_end]).ok()?;
        let size_str = size_str.split(';').next().unwrap_or("").trim();
        let chunk_size = usize::from_str_radix(size_str, 16).ok()?;
        pos += line_end + 2;

        if chunk_size == 0 {
            // last chunk: skip optional trailers, then \r\n
            if pos + 2 <= buf.len() && &buf[pos..pos + 2] == b"\r\n" {
                return Some(pos + 2);
            }
            return Some(pos);
        }

        if pos + chunk_size + 2 > buf.len() {
            return None;
        }
        pos += chunk_size + 2;
    }
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_get() {
        let raw = b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (req, consumed) = Parser::parse(raw).unwrap();
        assert_eq!(req.method, Method::GET);
        assert_eq!(req.path, "/hello");
        assert_eq!(consumed, raw.len());
        assert_eq!(req.header("host"), Some("localhost"));
    }

    #[test]
    fn test_parse_with_body() {
        let raw = b"POST /data HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let (req, consumed) = Parser::parse(raw).unwrap();
        assert_eq!(req.method, Method::POST);
        assert_eq!(req.body, b"hello");
        assert_eq!(consumed, raw.len());
    }

    #[test]
    fn test_parse_with_query() {
        let raw = b"GET /search?q=rust&lang=en HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let (req, _) = Parser::parse(raw).unwrap();
        assert_eq!(req.path, "/search");
        assert_eq!(req.query, "q=rust&lang=en");
    }

    #[test]
    fn test_incomplete_request() {
        let raw = b"GET /hello HTTP/1.1\r\n";
        assert!(matches!(Parser::parse(raw), Err(ParseError::Incomplete)));
    }

    #[test]
    fn test_invalid_request_line() {
        let raw = b"INVALID\r\n\r\n";
        assert!(matches!(
            Parser::parse(raw),
            Err(ParseError::InvalidRequestLine(_))
        ));
    }

    #[test]
    fn test_chunked_body() {
        let raw =
            b"POST /upload HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let (req, _) = Parser::parse(raw).unwrap();
        assert_eq!(req.body, b"hello");
    }
}

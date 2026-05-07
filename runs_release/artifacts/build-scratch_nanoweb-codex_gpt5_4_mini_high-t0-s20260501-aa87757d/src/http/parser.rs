use crate::error::NanowebError;
use crate::http::types::{Method, Version};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ParsedRequest {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: Option<String>,
    pub version: Version,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub connection_close: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Incomplete,
    Protocol(String),
}

pub fn parse_request(buf: &[u8]) -> Result<(ParsedRequest, usize), ParseError> {
    let header_end = find_header_end(buf).ok_or(ParseError::Incomplete)?;
    let head = &buf[..header_end];
    let head_str = std::str::from_utf8(head)
        .map_err(|_| ParseError::Protocol("request is not valid utf-8".into()))?;
    let mut lines = head_str.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| ParseError::Protocol("missing request line".into()))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| ParseError::Protocol("missing method".into()))?
        .parse()
        .map_err(ParseError::Protocol)?;
    let target = parts
        .next()
        .ok_or_else(|| ParseError::Protocol("missing target".into()))?
        .to_string();
    let version = match parts
        .next()
        .ok_or_else(|| ParseError::Protocol("missing version".into()))?
    {
        "HTTP/1.0" => Version::Http10,
        "HTTP/1.1" => Version::Http11,
        other => return Err(ParseError::Protocol(format!("unsupported version {other}"))),
    };
    if parts.next().is_some() {
        return Err(ParseError::Protocol("malformed request line".into()));
    }

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| ParseError::Protocol("malformed header".into()))?;
        let key = name.trim().to_ascii_lowercase();
        let val = value.trim().to_string();
        if headers.insert(key, val).is_some() {
            // Duplicate header names are allowed generally, but we keep last-value semantics.
        }
    }

    let (path, query) = parse_target(&target)?;
    let connection_close = match headers.get("connection").map(|v| v.to_ascii_lowercase()) {
        Some(value) if value.contains("close") => true,
        Some(value) if value.contains("keep-alive") => false,
        _ => matches!(version, Version::Http10),
    };

    let body_start = header_end + 4;
    let body = if let Some(te) = headers.get("transfer-encoding") {
        if te.to_ascii_lowercase().contains("chunked") {
            if headers.contains_key("content-length") {
                return Err(ParseError::Protocol(
                    "chunked request cannot also set content-length".into(),
                ));
            }
            let (decoded, consumed) = parse_chunked(&buf[body_start..])?;
            let consumed_total = body_start + consumed;
            let parsed = ParsedRequest {
                method,
                target,
                path,
                query,
                version,
                headers,
                body: decoded,
                connection_close,
            };
            return Ok((parsed, consumed_total));
        }
        return Err(ParseError::Protocol("unsupported transfer encoding".into()));
    } else if let Some(len) = headers.get("content-length") {
        let len: usize = len
            .parse()
            .map_err(|_| ParseError::Protocol("invalid content-length".into()))?;
        if buf.len() < body_start + len {
            return Err(ParseError::Incomplete);
        }
        buf[body_start..body_start + len].to_vec()
    } else {
        Vec::new()
    };
    let consumed = body_start + body.len();
    let parsed = ParsedRequest {
        method,
        target,
        path,
        query,
        version,
        headers,
        body,
        connection_close,
    };
    Ok((parsed, consumed))
}

fn parse_target(target: &str) -> Result<(String, Option<String>), ParseError> {
    if target == "*" {
        return Ok(("*".to_string(), None));
    }
    if let Some((path, query)) = target.split_once('?') {
        return Ok((path.to_string(), Some(query.to_string())));
    }
    Ok((target.to_string(), None))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_chunked(buf: &[u8]) -> Result<(Vec<u8>, usize), ParseError> {
    let mut cursor = 0usize;
    let mut body = Vec::new();
    loop {
        let size_end = find_line_end(&buf[cursor..]).ok_or(ParseError::Incomplete)?;
        let size_line = std::str::from_utf8(&buf[cursor..cursor + size_end])
            .map_err(|_| ParseError::Protocol("invalid chunk size line".into()))?;
        let size = size_line
            .split(';')
            .next()
            .ok_or_else(|| ParseError::Protocol("invalid chunk size".into()))?
            .trim();
        let size = usize::from_str_radix(size, 16)
            .map_err(|_| ParseError::Protocol("invalid chunk size".into()))?;
        cursor += size_end + 2;
        if size == 0 {
            if buf[cursor..].starts_with(b"\r\n") {
                cursor += 2;
                break;
            }
            let trailer_end = find_header_end(&buf[cursor..]).ok_or(ParseError::Incomplete)?;
            cursor += trailer_end + 4;
            break;
        }
        if buf.len() < cursor + size + 2 {
            return Err(ParseError::Incomplete);
        }
        body.extend_from_slice(&buf[cursor..cursor + size]);
        cursor += size + 2;
        if &buf[cursor - 2..cursor] != b"\r\n" {
            return Err(ParseError::Protocol("chunk missing terminator".into()));
        }
    }
    Ok((body, cursor))
}

fn find_line_end(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

impl From<ParseError> for NanowebError {
    fn from(value: ParseError) -> Self {
        NanowebError::Parse(match value {
            ParseError::Incomplete => "incomplete request".to_string(),
            ParseError::Protocol(s) => s,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_request;

    #[test]
    fn parse_chunked_body() {
        let raw = b"POST /submit HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        let (req, consumed) = parse_request(raw).unwrap();
        assert_eq!(consumed, raw.len());
        assert_eq!(req.body, b"Wikipedia");
    }
}

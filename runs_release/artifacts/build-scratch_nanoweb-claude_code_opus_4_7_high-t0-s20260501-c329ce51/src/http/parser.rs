use super::types::{HeaderMap, Method, Request, Version};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

pub type ParseResult<T> = Result<T, ParseError>;

const MAX_HEAD_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    None,
    ContentLength(usize),
    Chunked,
}

#[derive(Debug, Clone)]
pub struct ParsedHead {
    pub method: Method,
    pub target: String,
    pub version: Version,
    pub headers: HeaderMap,
    pub body_kind: BodyKind,
    pub keep_alive: bool,
    pub head_len: usize,
}

pub struct RequestParser;

impl RequestParser {
    /// Try to parse the request head from `buf`. Returns `Ok(Some(head))` if a
    /// complete head is present, `Ok(None)` if more data is required, or an
    /// `Err` for malformed requests.
    pub fn parse_head(buf: &[u8]) -> ParseResult<Option<ParsedHead>> {
        if buf.len() > MAX_HEAD_SIZE {
            return Err(ParseError("request head too large".into()));
        }
        let head_end = match find_double_crlf(buf) {
            Some(idx) => idx,
            None => return Ok(None),
        };
        let head = &buf[..head_end];
        let head_str =
            std::str::from_utf8(head).map_err(|_| ParseError("non-utf8 in request head".into()))?;

        let mut lines = head_str.split("\r\n");
        let request_line = lines
            .next()
            .ok_or_else(|| ParseError("missing request line".into()))?;
        let mut parts = request_line.split(' ');
        let method_s = parts
            .next()
            .ok_or_else(|| ParseError("missing method".into()))?;
        let target = parts
            .next()
            .ok_or_else(|| ParseError("missing target".into()))?;
        let version_s = parts
            .next()
            .ok_or_else(|| ParseError("missing http version".into()))?;
        if parts.next().is_some() {
            return Err(ParseError("malformed request line".into()));
        }
        let method = Method::from_str(method_s).map_err(ParseError)?;
        if target.is_empty() || !target.starts_with('/') && target != "*" {
            return Err(ParseError("invalid request target".into()));
        }
        let version = match version_s {
            "HTTP/1.0" => Version::Http10,
            "HTTP/1.1" => Version::Http11,
            other => return Err(ParseError(format!("unsupported version: {}", other))),
        };

        let mut headers = HeaderMap::new();
        for line in lines {
            if line.is_empty() {
                break;
            }
            let colon = line
                .find(':')
                .ok_or_else(|| ParseError("malformed header line".into()))?;
            let name = &line[..colon];
            if name.is_empty() || !is_valid_token(name) {
                return Err(ParseError("invalid header name".into()));
            }
            let value = line[colon + 1..].trim();
            headers.append(name, value);
        }

        let mut body_kind = BodyKind::None;
        let te = headers.get("Transfer-Encoding").map(|s| s.to_string());
        let cl = headers.get("Content-Length").map(|s| s.to_string());
        if let Some(te) = te {
            if te.to_ascii_lowercase().contains("chunked") {
                if cl.is_some() {
                    return Err(ParseError(
                        "both Content-Length and Transfer-Encoding present".into(),
                    ));
                }
                body_kind = BodyKind::Chunked;
            } else {
                return Err(ParseError(format!("unsupported Transfer-Encoding: {}", te)));
            }
        } else if let Some(cl) = cl {
            let n: usize = cl
                .trim()
                .parse()
                .map_err(|_| ParseError("invalid Content-Length".into()))?;
            body_kind = BodyKind::ContentLength(n);
        }

        let keep_alive = match version {
            Version::Http11 => headers
                .get("Connection")
                .map(|c| !c.eq_ignore_ascii_case("close"))
                .unwrap_or(true),
            Version::Http10 => headers
                .get("Connection")
                .map(|c| c.eq_ignore_ascii_case("keep-alive"))
                .unwrap_or(false),
        };

        Ok(Some(ParsedHead {
            method,
            target: target.to_string(),
            version,
            headers,
            body_kind,
            keep_alive,
            head_len: head_end + 4,
        }))
    }

    /// Decode a chunked body, returning (decoded_body, total_consumed). Returns
    /// `Ok(None)` if more data is needed.
    pub fn parse_chunked(buf: &[u8]) -> ParseResult<Option<(Vec<u8>, usize)>> {
        let mut out = Vec::new();
        let mut pos = 0;
        loop {
            // size line
            let line_end = match find_crlf(&buf[pos..]) {
                Some(i) => i,
                None => return Ok(None),
            };
            let size_line = std::str::from_utf8(&buf[pos..pos + line_end])
                .map_err(|_| ParseError("invalid chunk size line".into()))?;
            // strip extensions after ';'
            let size_str = size_line.split(';').next().unwrap().trim();
            let size = usize::from_str_radix(size_str, 16)
                .map_err(|_| ParseError("invalid chunk size".into()))?;
            pos += line_end + 2;
            if size == 0 {
                // optional trailers terminated by CRLF
                // require terminating CRLF
                if buf.len() < pos + 2 {
                    return Ok(None);
                }
                if &buf[pos..pos + 2] == b"\r\n" {
                    return Ok(Some((out, pos + 2)));
                }
                // skip trailers until empty line
                loop {
                    let le = match find_crlf(&buf[pos..]) {
                        Some(i) => i,
                        None => return Ok(None),
                    };
                    if le == 0 {
                        return Ok(Some((out, pos + 2)));
                    }
                    pos += le + 2;
                }
            }
            if buf.len() < pos + size + 2 {
                return Ok(None);
            }
            out.extend_from_slice(&buf[pos..pos + size]);
            pos += size;
            if &buf[pos..pos + 2] != b"\r\n" {
                return Err(ParseError("malformed chunk terminator".into()));
            }
            pos += 2;
        }
    }
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    for i in 0..=buf.len() - 4 {
        if &buf[i..i + 4] == b"\r\n\r\n" {
            return Some(i);
        }
    }
    None
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    if buf.len() < 2 {
        return None;
    }
    for i in 0..=buf.len() - 2 {
        if &buf[i..i + 2] == b"\r\n" {
            return Some(i);
        }
    }
    None
}

fn is_valid_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| {
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

/// Build the full Request from a ParsedHead and a body buffer. Splits target
/// into path/query.
pub fn build_request(head: ParsedHead, body: Vec<u8>) -> Request {
    let mut req = Request::new(head.method, head.target, head.version);
    req.headers = head.headers;
    req.body = body;
    req
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_get() {
        let buf = b"GET /foo HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let head = RequestParser::parse_head(buf).unwrap().unwrap();
        assert_eq!(head.method, Method::Get);
        assert_eq!(head.target, "/foo");
        assert_eq!(head.version, Version::Http11);
        assert_eq!(head.body_kind, BodyKind::None);
        assert!(head.keep_alive);
    }

    #[test]
    fn parse_post_content_length() {
        let buf = b"POST /a HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhello";
        let head = RequestParser::parse_head(buf).unwrap().unwrap();
        assert_eq!(head.body_kind, BodyKind::ContentLength(5));
    }

    #[test]
    fn parse_partial_returns_none() {
        let buf = b"GET / HTTP/1.1\r\nHost: x\r\n";
        assert!(RequestParser::parse_head(buf).unwrap().is_none());
    }

    #[test]
    fn parse_invalid_method_rejected() {
        let buf = b"FOO / HTTP/1.1\r\n\r\n";
        assert!(RequestParser::parse_head(buf).is_err());
    }

    #[test]
    fn parse_invalid_version_rejected() {
        let buf = b"GET / HTTP/9.9\r\n\r\n";
        assert!(RequestParser::parse_head(buf).is_err());
    }

    #[test]
    fn parse_chunked_simple() {
        let buf = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let (out, used) = RequestParser::parse_chunked(buf).unwrap().unwrap();
        assert_eq!(out, b"hello world");
        assert_eq!(used, buf.len());
    }

    #[test]
    fn parse_chunked_partial() {
        let buf = b"5\r\nhel";
        assert!(RequestParser::parse_chunked(buf).unwrap().is_none());
    }

    #[test]
    fn rejects_both_te_and_cl() {
        let buf = b"POST / HTTP/1.1\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert!(RequestParser::parse_head(buf).is_err());
    }
}

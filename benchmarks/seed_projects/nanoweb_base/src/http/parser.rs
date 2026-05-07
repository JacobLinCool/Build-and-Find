//! HTTP/1.1 message parser.
//!
//! Implements only what is needed for this framework:
//! * request-line (method SP target SP version CRLF)
//! * headers terminated by CRLF CRLF
//! * bodies framed by `Content-Length` or `Transfer-Encoding: chunked`
//!
//! The parser intentionally has **no** dependency on the router, the
//! middleware chain, or the typed-body helpers.

use crate::error::NanowebError;
use crate::http::types::{Headers, Method, Request, Version};

/// Limit on the combined start-line + headers size. Requests whose head
/// exceeds this are rejected with 431-ish parse errors.
pub const MAX_HEADER_BYTES: usize = 16 * 1024;

/// Result of one parse attempt against a byte buffer.
#[derive(Debug)]
pub enum ParseOutcome {
    /// Not enough bytes yet; call again when more data arrives.
    Incomplete,
    /// A full request has been materialised. `consumed` is the number
    /// of bytes the parser ate from the input buffer.
    Complete { request: Request, consumed: usize },
}

/// Attempt to parse a full HTTP/1.1 request out of `buf`.
pub fn parse_request(buf: &[u8]) -> Result<ParseOutcome, NanowebError> {
    let head_end = match find_double_crlf(buf) {
        Some(i) => i,
        None => {
            if buf.len() > MAX_HEADER_BYTES {
                return Err(NanowebError::Parse("header section too large".into()));
            }
            return Ok(ParseOutcome::Incomplete);
        }
    };

    let head = std::str::from_utf8(&buf[..head_end])
        .map_err(|_| NanowebError::Parse("non-utf8 in request head".into()))?;

    let mut lines = head.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| NanowebError::Parse("empty request".into()))?;
    let (method, target, version) = parse_request_line(request_line)?;

    let mut headers = Headers::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = parse_header_line(line)?;
        headers.append(name, value);
    }

    let body_start = head_end + 4; // skip the trailing CRLFCRLF
    let (body, body_consumed) = read_body(&headers, &buf[body_start..])?;
    let consumed = match body_consumed {
        Some(n) => body_start + n,
        None => return Ok(ParseOutcome::Incomplete),
    };

    let mut req = Request::new(method, target);
    req.version = version;
    req.headers = headers;
    req.body = body;

    Ok(ParseOutcome::Complete {
        request: req,
        consumed,
    })
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_request_line(line: &str) -> Result<(Method, String, Version), NanowebError> {
    let mut parts = line.splitn(3, ' ');
    let method = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing method".into()))?;
    let target = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing request target".into()))?;
    let version = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing http version".into()))?;

    let method = Method::parse(method)
        .ok_or_else(|| NanowebError::Parse(format!("unknown method: {}", method)))?;
    let version = Version::parse(version)
        .ok_or_else(|| NanowebError::Parse(format!("unsupported version: {}", version)))?;
    if version == Version::Http10 {
        // Still parse but framework behaviour is HTTP/1.1-only.
    }
    Ok((method, target.to_string(), version))
}

fn parse_header_line(line: &str) -> Result<(String, String), NanowebError> {
    let colon = line
        .find(':')
        .ok_or_else(|| NanowebError::Parse(format!("malformed header: {}", line)))?;
    let name = line[..colon].trim();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Err(NanowebError::Parse("bad header name".into()));
    }
    let value = line[colon + 1..].trim();
    Ok((name.to_ascii_lowercase(), value.to_string()))
}

fn read_body(
    headers: &Headers,
    body_bytes: &[u8],
) -> Result<(Vec<u8>, Option<usize>), NanowebError> {
    let chunked = headers
        .get("transfer-encoding")
        .map(|v| v.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false);
    if chunked {
        return read_chunked(body_bytes);
    }
    match headers.get("content-length") {
        Some(raw) => {
            let n: usize = raw
                .trim()
                .parse()
                .map_err(|_| NanowebError::Parse("bad content-length".into()))?;
            if body_bytes.len() < n {
                Ok((Vec::new(), None))
            } else {
                Ok((body_bytes[..n].to_vec(), Some(n)))
            }
        }
        None => Ok((Vec::new(), Some(0))),
    }
}

fn read_chunked(buf: &[u8]) -> Result<(Vec<u8>, Option<usize>), NanowebError> {
    let mut pos = 0;
    let mut out = Vec::new();
    loop {
        let nl = match find_crlf(&buf[pos..]) {
            Some(i) => i,
            None => return Ok((Vec::new(), None)),
        };
        let size_line = std::str::from_utf8(&buf[pos..pos + nl])
            .map_err(|_| NanowebError::Parse("bad chunk size line".into()))?;
        let size_str = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_str, 16)
            .map_err(|_| NanowebError::Parse(format!("bad chunk size: {}", size_str)))?;
        pos += nl + 2;
        if size == 0 {
            // trailing CRLF after last-chunk (optional trailer headers
            // ignored for simplicity — they are rare in HTTP/1.1).
            if buf.len() < pos + 2 {
                return Ok((Vec::new(), None));
            }
            pos += 2;
            return Ok((out, Some(pos)));
        }
        if buf.len() < pos + size + 2 {
            return Ok((Vec::new(), None));
        }
        out.extend_from_slice(&buf[pos..pos + size]);
        pos += size + 2; // skip the CRLF after the chunk payload
    }
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_get() {
        let raw = b"GET /foo?x=1 HTTP/1.1\r\nHost: example\r\n\r\n";
        match parse_request(raw).unwrap() {
            ParseOutcome::Complete { request, consumed } => {
                assert_eq!(request.method.as_str(), "GET");
                assert_eq!(request.path, "/foo");
                assert_eq!(request.query, "x=1");
                assert_eq!(request.headers.get("host"), Some("example"));
                assert_eq!(consumed, raw.len());
            }
            other => panic!("expected complete, got {:?}", other),
        }
    }

    #[test]
    fn parses_content_length_body() {
        let raw = b"POST /p HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        match parse_request(raw).unwrap() {
            ParseOutcome::Complete { request, consumed } => {
                assert_eq!(request.body, b"hello");
                assert_eq!(consumed, raw.len());
            }
            _ => panic!("incomplete"),
        }
    }

    #[test]
    fn incomplete_when_body_missing() {
        let raw = b"POST /p HTTP/1.1\r\nContent-Length: 5\r\n\r\nhi";
        assert!(matches!(
            parse_request(raw).unwrap(),
            ParseOutcome::Incomplete
        ));
    }

    #[test]
    fn parses_chunked_body() {
        let raw = b"POST /p HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        match parse_request(raw).unwrap() {
            ParseOutcome::Complete { request, .. } => {
                assert_eq!(request.body, b"hello world");
            }
            _ => panic!("incomplete"),
        }
    }

    #[test]
    fn rejects_bad_method() {
        let raw = b"NOPE / HTTP/1.1\r\n\r\n";
        assert!(parse_request(raw).is_err());
    }
}

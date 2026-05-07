use std::str::FromStr;

use tokio::io::{AsyncRead, AsyncReadExt};

use crate::error::{NanowebError, Result};
use crate::http::{HeaderMap, Method, Request};

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

pub async fn read_request<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Option<Request>> {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0_u8; 4096];

    loop {
        if buf.len() > MAX_HEADER_BYTES + MAX_BODY_BYTES {
            return Err(NanowebError::Parse("request too large".into()));
        }
        match parse_request(&buf)? {
            ParseState::Complete(req, _) => return Ok(Some(req)),
            ParseState::Incomplete => {
                let n = reader.read(&mut tmp).await?;
                if n == 0 {
                    return if buf.is_empty() {
                        Ok(None)
                    } else {
                        Err(NanowebError::Parse("unexpected EOF".into()))
                    };
                }
                buf.extend_from_slice(&tmp[..n]);
            }
        }
    }
}

#[derive(Debug)]
pub enum ParseState {
    Complete(Request, usize),
    Incomplete,
}

pub fn parse_request(bytes: &[u8]) -> Result<ParseState> {
    let Some(header_end) = find_header_end(bytes) else {
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(NanowebError::Parse("headers too large".into()));
        }
        return Ok(ParseState::Incomplete);
    };

    let header_text = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| NanowebError::Parse("headers are not valid UTF-8".into()))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| NanowebError::Parse("missing request line".into()))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing method".into()))
        .and_then(|m| Method::from_str(m).map_err(NanowebError::Parse))?;
    let target = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing target".into()))?;
    let version = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing version".into()))?;
    if parts.next().is_some() || version != "HTTP/1.1" || !target.starts_with('/') {
        return Err(NanowebError::Parse("invalid request line".into()));
    }

    let mut headers = HeaderMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| NanowebError::Parse("malformed header".into()))?;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(NanowebError::Parse("invalid header name".into()));
        }
        headers.insert(name.to_ascii_lowercase(), value.trim().to_string());
    }

    let body_start = header_end + 4;
    let transfer_encoding = headers
        .get("transfer-encoding")
        .map(|v| v.to_ascii_lowercase())
        .unwrap_or_default();
    let body = if transfer_encoding
        .split(',')
        .any(|v| v.trim().eq_ignore_ascii_case("chunked"))
    {
        match parse_chunked(&bytes[body_start..])? {
            Some((body, consumed)) => {
                return complete(
                    method,
                    target,
                    version,
                    headers,
                    body,
                    body_start + consumed,
                );
            }
            None => return Ok(ParseState::Incomplete),
        }
    } else if let Some(length) = headers.get("content-length") {
        let length = length
            .parse::<usize>()
            .map_err(|_| NanowebError::Parse("invalid content-length".into()))?;
        if length > MAX_BODY_BYTES {
            return Err(NanowebError::Parse("body too large".into()));
        }
        if bytes.len() < body_start + length {
            return Ok(ParseState::Incomplete);
        }
        bytes[body_start..body_start + length].to_vec()
    } else {
        Vec::new()
    };

    let consumed = body_start
        + headers
            .get("content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
    complete(method, target, version, headers, body, consumed)
}

fn complete(
    method: Method,
    target: &str,
    version: &str,
    headers: HeaderMap,
    body: Vec<u8>,
    consumed: usize,
) -> Result<ParseState> {
    let mut req = Request::new(method, target);
    req.version = version.to_string();
    req.headers = headers;
    req.body = body;
    Ok(ParseState::Complete(req, consumed))
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_chunked(bytes: &[u8]) -> Result<Option<(Vec<u8>, usize)>> {
    let mut pos = 0;
    let mut body = Vec::new();
    loop {
        let Some(line_end) = find_crlf(&bytes[pos..]) else {
            return Ok(None);
        };
        let line = std::str::from_utf8(&bytes[pos..pos + line_end])
            .map_err(|_| NanowebError::Parse("invalid chunk size".into()))?;
        let size_part = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_part, 16)
            .map_err(|_| NanowebError::Parse("invalid chunk size".into()))?;
        pos += line_end + 2;
        if body.len() + size > MAX_BODY_BYTES {
            return Err(NanowebError::Parse("body too large".into()));
        }
        if size == 0 {
            if bytes.len() < pos + 2 {
                return Ok(None);
            }
            if &bytes[pos..pos + 2] != b"\r\n" {
                return Err(NanowebError::Parse("malformed final chunk".into()));
            }
            return Ok(Some((body, pos + 2)));
        }
        if bytes.len() < pos + size + 2 {
            return Ok(None);
        }
        body.extend_from_slice(&bytes[pos..pos + size]);
        pos += size;
        if &bytes[pos..pos + 2] != b"\r\n" {
            return Err(NanowebError::Parse("malformed chunk".into()));
        }
        pos += 2;
    }
}

fn find_crlf(bytes: &[u8]) -> Option<usize> {
    bytes.windows(2).position(|w| w == b"\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_length() {
        let raw = b"POST /x HTTP/1.1\r\nHost: example\r\nContent-Length: 4\r\n\r\ntest";
        let ParseState::Complete(req, _) = parse_request(raw).unwrap() else {
            panic!("expected complete request");
        };
        assert_eq!(req.body, b"test");
        assert_eq!(req.header("host"), Some("example"));
    }

    #[test]
    fn parses_chunked() {
        let raw = b"POST /x HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        let ParseState::Complete(req, _) = parse_request(raw).unwrap() else {
            panic!("expected complete request");
        };
        assert_eq!(req.body, b"Wikipedia");
    }

    #[test]
    fn rejects_bad_request_line() {
        let raw = b"GET * HTTP/1.0\r\n\r\n";
        assert!(parse_request(raw).is_err());
    }
}

use super::{HeaderMap, Method, Request};
use crate::error::{NanowebError, Result};
use std::str;

const MAX_HEADERS: usize = 64 * 1024;
const MAX_BODY: usize = 10 * 1024 * 1024;

pub fn parse_request(buf: &[u8]) -> Result<Option<(Request, usize)>> {
    let header_end = match find_header_end(buf) {
        Some(pos) => pos,
        None if buf.len() > MAX_HEADERS => {
            return Err(NanowebError::BadRequest("headers too large".into()));
        }
        None => return Ok(None),
    };
    let head = str::from_utf8(&buf[..header_end])
        .map_err(|_| NanowebError::BadRequest("headers are not valid utf-8".into()))?;
    let mut lines = head.split("\r\n");
    let line = lines
        .next()
        .ok_or_else(|| NanowebError::BadRequest("missing request line".into()))?;
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| NanowebError::BadRequest("missing method".into()))?
        .parse::<Method>()
        .map_err(|_| NanowebError::BadRequest("invalid method".into()))?;
    let target = parts
        .next()
        .ok_or_else(|| NanowebError::BadRequest("missing target".into()))?;
    let version = parts
        .next()
        .ok_or_else(|| NanowebError::BadRequest("missing version".into()))?;
    if parts.next().is_some() || version != "HTTP/1.1" || !target.starts_with('/') {
        return Err(NanowebError::BadRequest("invalid request line".into()));
    }

    let mut headers = HeaderMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| NanowebError::BadRequest("malformed header".into()))?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(NanowebError::BadRequest("invalid header name".into()));
        }
        headers.insert(name.to_ascii_lowercase(), value.trim().to_string());
    }

    let body_start = header_end + 4;
    let transfer = headers
        .get("transfer-encoding")
        .map(|v| v.to_ascii_lowercase());
    let (body, consumed) = if transfer.as_deref().is_some_and(|v| v.contains("chunked")) {
        match parse_chunked(&buf[body_start..])? {
            Some((body, used)) => (body, body_start + used),
            None => return Ok(None),
        }
    } else if let Some(len) = headers.get("content-length") {
        let len = len
            .parse::<usize>()
            .map_err(|_| NanowebError::BadRequest("invalid content-length".into()))?;
        if len > MAX_BODY {
            return Err(NanowebError::BadRequest("body too large".into()));
        }
        if buf.len() < body_start + len {
            return Ok(None);
        }
        (buf[body_start..body_start + len].to_vec(), body_start + len)
    } else {
        (Vec::new(), body_start)
    };

    let mut request = Request::new(method, target);
    request.version = version.to_string();
    request.headers = headers;
    request.body = body;
    Ok(Some((request, consumed)))
}

fn parse_chunked(buf: &[u8]) -> Result<Option<(Vec<u8>, usize)>> {
    let mut pos = 0;
    let mut body = Vec::new();
    loop {
        let line_end = match find_crlf(&buf[pos..]) {
            Some(n) => pos + n,
            None => return Ok(None),
        };
        let line = str::from_utf8(&buf[pos..line_end])
            .map_err(|_| NanowebError::BadRequest("invalid chunk size".into()))?;
        let size_part = line.split(';').next().unwrap_or(line);
        let size = usize::from_str_radix(size_part.trim(), 16)
            .map_err(|_| NanowebError::BadRequest("invalid chunk size".into()))?;
        pos = line_end + 2;
        if size == 0 {
            let trailer_end = match find_header_end(&buf[pos..]) {
                Some(n) => pos + n + 4,
                None if buf.get(pos..pos + 2) == Some(b"\r\n") => pos + 2,
                None => return Ok(None),
            };
            return Ok(Some((body, trailer_end)));
        }
        if body.len() + size > MAX_BODY {
            return Err(NanowebError::BadRequest("body too large".into()));
        }
        if buf.len() < pos + size + 2 {
            return Ok(None);
        }
        body.extend_from_slice(&buf[pos..pos + size]);
        if &buf[pos + size..pos + size + 2] != b"\r\n" {
            return Err(NanowebError::BadRequest("chunk missing terminator".into()));
        }
        pos += size + 2;
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_length() {
        let raw = b"POST /x HTTP/1.1\r\nHost: localhost\r\nContent-Length: 3\r\n\r\nabc";
        let (req, used) = parse_request(raw).unwrap().unwrap();
        assert_eq!(used, raw.len());
        assert_eq!(req.body, b"abc");
    }

    #[test]
    fn parses_chunked() {
        let raw = b"POST /x HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n";
        let (req, _) = parse_request(raw).unwrap().unwrap();
        assert_eq!(req.body, b"abc");
    }

    #[test]
    fn rejects_bad_request_line() {
        assert!(parse_request(b"GET / HTTP/1.0\r\n\r\n").is_err());
    }
}

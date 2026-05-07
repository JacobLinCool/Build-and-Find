use crate::error::NanowebError;
use crate::http::types::{Method, Request};
use std::collections::HashMap;

pub fn parse_request(input: &[u8]) -> Result<(Request, usize), NanowebError> {
    let header_end =
        find_headers_end(input).ok_or_else(|| NanowebError::parse("incomplete headers"))?;
    let head = &input[..header_end];
    let mut lines = head.split(|b| *b == b'\n').map(|l| trim_cr(l));
    let request_line = lines
        .next()
        .ok_or_else(|| NanowebError::parse("missing request line"))?;
    let request_line = std::str::from_utf8(request_line)
        .map_err(|_| NanowebError::parse("invalid request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| NanowebError::parse("missing method"))?;
    let target = parts
        .next()
        .ok_or_else(|| NanowebError::parse("missing target"))?;
    let version = parts
        .next()
        .ok_or_else(|| NanowebError::parse("missing version"))?;
    if parts.next().is_some() || !version.starts_with("HTTP/1.1") {
        return Err(NanowebError::parse("invalid request line"));
    }
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let s = std::str::from_utf8(line).map_err(|_| NanowebError::parse("invalid header"))?;
        let (name, value) = s
            .split_once(':')
            .ok_or_else(|| NanowebError::parse("malformed header"))?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    let body_start = header_end + 4;
    let body;
    let consumed;
    if let Some(te) = headers.get("transfer-encoding") {
        if te.to_ascii_lowercase().contains("chunked") {
            let (decoded, used) = parse_chunked(&input[body_start..])?;
            body = decoded;
            consumed = body_start + used;
        } else {
            return Err(NanowebError::parse("unsupported transfer encoding"));
        }
    } else if let Some(cl) = headers.get("content-length") {
        let len: usize = cl
            .parse()
            .map_err(|_| NanowebError::parse("invalid content-length"))?;
        if input.len() < body_start + len {
            return Err(NanowebError::parse("incomplete body"));
        }
        body = input[body_start..body_start + len].to_vec();
        consumed = body_start + len;
    } else {
        body = Vec::new();
        consumed = body_start;
    }
    Ok((
        Request {
            method: Method::from(method),
            target: target.to_string(),
            version: version.to_string(),
            headers,
            body,
            params: HashMap::new(),
            peer_addr: None,
            request_id: String::new(),
        },
        consumed,
    ))
}

fn find_headers_end(input: &[u8]) -> Option<usize> {
    input.windows(4).position(|w| w == b"\r\n\r\n")
}

fn trim_cr(line: &[u8]) -> &[u8] {
    if line.ends_with(b"\r") {
        &line[..line.len() - 1]
    } else {
        line
    }
}

fn parse_chunked(input: &[u8]) -> Result<(Vec<u8>, usize), NanowebError> {
    let mut pos = 0;
    let mut out = Vec::new();
    loop {
        let line_end =
            find_crlf(&input[pos..]).ok_or_else(|| NanowebError::parse("incomplete chunk size"))?;
        let size_line = std::str::from_utf8(&input[pos..pos + line_end])
            .map_err(|_| NanowebError::parse("invalid chunk size"))?;
        let size = usize::from_str_radix(size_line.split(';').next().unwrap().trim(), 16)
            .map_err(|_| NanowebError::parse("invalid chunk size"))?;
        pos += line_end + 2;
        if size == 0 {
            let end = find_double_crlf(&input[pos..])
                .map(|n| pos + n + 4)
                .unwrap_or(pos + 2);
            return Ok((out, end));
        }
        if input.len() < pos + size + 2 {
            return Err(NanowebError::parse("incomplete chunk"));
        }
        out.extend_from_slice(&input[pos..pos + size]);
        pos += size;
        if &input[pos..pos + 2] != b"\r\n" {
            return Err(NanowebError::parse("invalid chunk terminator"));
        }
        pos += 2;
    }
}
fn find_crlf(input: &[u8]) -> Option<usize> {
    input.windows(2).position(|w| w == b"\r\n")
}
fn find_double_crlf(input: &[u8]) -> Option<usize> {
    input.windows(4).position(|w| w == b"\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_content_length() {
        let raw = b"GET / HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let (req, used) = parse_request(raw).unwrap();
        assert_eq!(req.body, b"hello");
        assert_eq!(used, raw.len());
    }
    #[test]
    fn parses_chunked() {
        let raw = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let (req, _) = parse_request(raw).unwrap();
        assert_eq!(req.body, b"hello");
    }
}

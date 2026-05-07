use super::types::{Method, Request};
use crate::error::HttpError;
use std::collections::HashMap;

const MAX_HEADER_SIZE: usize = 8192;
const MAX_BODY_SIZE: usize = 10 * 1024 * 1024; // 10MB

pub fn parse_request(
    data: &[u8],
    addr: std::net::SocketAddr,
    request_id: String,
) -> Result<Request, HttpError> {
    let header_end =
        find_header_end(data).ok_or_else(|| HttpError::bad_request("missing header terminator"))?;
    let header_bytes = &data[..header_end];
    let body_start = header_end + 4; // \r\n\r\n

    let header_str = std::str::from_utf8(header_bytes)
        .map_err(|_| HttpError::bad_request("invalid UTF-8 in headers"))?;

    let mut lines = header_str.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| HttpError::bad_request("empty request"))?;

    let (method, path, query, version) = parse_request_line(request_line)?;

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let colon = line
            .find(':')
            .ok_or_else(|| HttpError::bad_request("malformed header"))?;
        let name = line[..colon].trim().to_ascii_lowercase();
        let value = line[colon + 1..].trim().to_string();
        headers.insert(name, value);
    }

    let body = if data.len() > body_start {
        &data[body_start..]
    } else {
        &[]
    };

    let body = if headers
        .get("transfer-encoding")
        .map(|s| s.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false)
    {
        parse_chunked(body)?
    } else if let Some(len) = headers.get("content-length") {
        let len: usize = len
            .parse()
            .map_err(|_| HttpError::bad_request("invalid content-length"))?;
        if len > MAX_BODY_SIZE {
            return Err(HttpError::bad_request("body too large"));
        }
        if body.len() < len {
            return Err(HttpError::bad_request("incomplete body"));
        }
        body[..len].to_vec()
    } else {
        Vec::new()
    };

    Ok(Request {
        method,
        path,
        query,
        version,
        headers,
        body,
        params: HashMap::new(),
        addr,
        request_id,
    })
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if &data[i..i + 4] == b"\r\n\r\n" {
            if i > MAX_HEADER_SIZE {
                return None;
            }
            return Some(i);
        }
    }
    None
}

fn parse_request_line(line: &str) -> Result<(Method, String, Option<String>, String), HttpError> {
    let mut parts = line.splitn(3, ' ');
    let method_str = parts
        .next()
        .ok_or_else(|| HttpError::bad_request("missing method"))?;
    let target = parts
        .next()
        .ok_or_else(|| HttpError::bad_request("missing target"))?;
    let version = parts
        .next()
        .ok_or_else(|| HttpError::bad_request("missing version"))?;

    let method = Method::from_bytes(method_str.as_bytes())
        .ok_or_else(|| HttpError::bad_request("unsupported method"))?;

    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(HttpError::bad_request("unsupported HTTP version"));
    }

    let (path, query) = match target.find('?') {
        Some(i) => (target[..i].to_string(), Some(target[i + 1..].to_string())),
        None => (target.to_string(), None),
    };

    Ok((method, path, query, version.to_string()))
}

fn parse_chunked(data: &[u8]) -> Result<Vec<u8>, HttpError> {
    let mut result = Vec::new();
    let mut pos = 0;

    loop {
        let line_end = data[pos..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| HttpError::bad_request("invalid chunked encoding"))?;

        let size_str = std::str::from_utf8(&data[pos..pos + line_end])
            .map_err(|_| HttpError::bad_request("invalid chunk size"))?;
        let chunk_size = usize::from_str_radix(size_str.trim(), 16)
            .map_err(|_| HttpError::bad_request("invalid chunk size"))?;

        pos += line_end + 2;

        if chunk_size == 0 {
            break;
        }

        if pos + chunk_size > data.len() {
            return Err(HttpError::bad_request("incomplete chunked body"));
        }

        result.extend_from_slice(&data[pos..pos + chunk_size]);
        pos += chunk_size + 2; // skip trailing \r\n

        if result.len() > MAX_BODY_SIZE {
            return Err(HttpError::bad_request("body too large"));
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn test_addr() -> SocketAddr {
        "127.0.0.1:12345".parse().unwrap()
    }

    #[test]
    fn parse_simple_get() {
        let raw = b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = parse_request(raw, test_addr(), "test-1".into()).unwrap();
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.path, "/hello");
        assert_eq!(req.header("host"), Some("localhost"));
    }

    #[test]
    fn parse_post_with_body() {
        let raw = b"POST /data HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let req = parse_request(raw, test_addr(), "test-2".into()).unwrap();
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.body, b"hello");
    }

    #[test]
    fn parse_chunked() {
        let raw =
            b"POST /chunked HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let req = parse_request(raw, test_addr(), "test-3".into()).unwrap();
        assert_eq!(req.body, b"hello");
    }

    #[test]
    fn parse_query_string() {
        let raw = b"GET /search?q=rust&lang=en HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = parse_request(raw, test_addr(), "test-4".into()).unwrap();
        assert_eq!(req.path, "/search");
        assert_eq!(req.query.as_deref(), Some("q=rust&lang=en"));
        let params = req.query_params();
        assert_eq!(params.get("q").map(|s| s.as_str()), Some("rust"));
    }

    #[test]
    fn reject_bad_request() {
        let raw = b"GARBAGE\r\n\r\n";
        assert!(parse_request(raw, test_addr(), "test-5".into()).is_err());
    }
}

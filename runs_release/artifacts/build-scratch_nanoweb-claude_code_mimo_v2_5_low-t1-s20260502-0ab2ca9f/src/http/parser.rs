use super::types::{Headers, Method, Request};
use crate::error::{NanowebError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyEncoding {
    ContentLength(usize),
    Chunked,
    None,
}

pub fn parse_request(buf: &[u8]) -> Result<(Request, usize)> {
    let headers_end = find_headers_end(buf)
        .ok_or_else(|| NanowebError::Http("Could not find end of headers".into()))?;

    let header_bytes = &buf[..headers_end];
    let (method, path, version) = parse_request_line(header_bytes)?;

    let headers = parse_headers(header_bytes)?;
    let body_encoding = determine_body_encoding(&headers)?;
    let (body, consumed) = read_body(buf, headers_end + 4, body_encoding, &headers)?;

    let (base_path, query) = super::types::parse_query_string(&path);

    let mut request = Request::new(method, base_path);
    request.version = version;
    request.headers = headers;
    request.body = body;
    request.query = query;

    Ok((request, consumed))
}

fn find_headers_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_request_line(header_bytes: &[u8]) -> Result<(Method, String, String)> {
    let first_line_end = header_bytes
        .windows(2)
        .position(|w| w == b"\r\n")
        .ok_or_else(|| NanowebError::Http("No CRLF in request line".into()))?;
    let first_line = &header_bytes[..first_line_end];
    let mut parts = first_line.split(|&b| b == b' ');

    let method_bytes = parts
        .next()
        .ok_or_else(|| NanowebError::Http("Missing method".into()))?;
    let path_bytes = parts
        .next()
        .ok_or_else(|| NanowebError::Http("Missing path".into()))?;
    let version_bytes = parts
        .next()
        .ok_or_else(|| NanowebError::Http("Missing version".into()))?;

    let method_str = std::str::from_utf8(method_bytes)
        .map_err(|_| NanowebError::Http("Invalid UTF-8 in method".into()))?;
    let path_str = std::str::from_utf8(path_bytes)
        .map_err(|_| NanowebError::Http("Invalid UTF-8 in path".into()))?;
    let version_str = std::str::from_utf8(version_bytes)
        .map_err(|_| NanowebError::Http("Invalid UTF-8 in version".into()))?;

    let method = Method::from_str(method_str)
        .ok_or_else(|| NanowebError::Http(format!("Unknown method: {}", method_str)))?;

    if !version_str.starts_with("HTTP/1.") {
        return Err(NanowebError::Http(format!(
            "Unsupported HTTP version: {}",
            version_str
        )));
    }

    Ok((method, path_str.to_string(), version_str.to_string()))
}

fn parse_headers(header_bytes: &[u8]) -> Result<Headers> {
    let mut headers = Headers::new();
    let first_line_end = header_bytes
        .windows(2)
        .position(|w| w == b"\r\n")
        .ok_or_else(|| NanowebError::Http("No CRLF in request line".into()))?;

    let remaining = &header_bytes[first_line_end + 2..];
    for line in remaining.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let colon = line.iter().position(|&b| b == b':').ok_or_else(|| {
            NanowebError::Http(format!(
                "Invalid header line: {:?}",
                std::str::from_utf8(line).unwrap_or("<non-utf8>")
            ))
        })?;
        let name = std::str::from_utf8(&line[..colon])
            .map_err(|_| NanowebError::Http("Invalid UTF-8 in header name".into()))?;
        let value = std::str::from_utf8(&line[colon + 1..])
            .map_err(|_| NanowebError::Http("Invalid UTF-8 in header value".into()))?;
        let value = value.trim();
        headers.insert(name.to_string(), value.to_string());
    }

    Ok(headers)
}

fn determine_body_encoding(headers: &Headers) -> Result<BodyEncoding> {
    if headers.is_chunked() {
        return Ok(BodyEncoding::Chunked);
    }
    if let Some(len) = headers.content_length() {
        return Ok(BodyEncoding::ContentLength(len));
    }
    Ok(BodyEncoding::None)
}

fn read_body(
    buf: &[u8],
    start: usize,
    encoding: BodyEncoding,
    _headers: &Headers,
) -> Result<(Vec<u8>, usize)> {
    match encoding {
        BodyEncoding::ContentLength(len) => {
            let total = start + len;
            if buf.len() < total {
                return Err(NanowebError::Http(format!(
                    "Body incomplete: expected {} bytes, got {}",
                    len,
                    buf.len() - start
                )));
            }
            let max_body = 10 * 1024 * 1024;
            if len > max_body {
                return Err(NanowebError::Http(format!(
                    "Body too large: {} exceeds {} byte limit",
                    len, max_body
                )));
            }
            Ok((buf[start..total].to_vec(), total))
        }
        BodyEncoding::Chunked => {
            let mut body = Vec::new();
            let mut pos = start;
            loop {
                if pos >= buf.len() {
                    return Err(NanowebError::Http("Incomplete chunked body".into()));
                }
                let line_end = buf[pos..]
                    .windows(2)
                    .position(|w| w == b"\r\n")
                    .ok_or_else(|| NanowebError::Http("Incomplete chunk size".into()))?;
                let size_str = std::str::from_utf8(&buf[pos..pos + line_end])
                    .map_err(|_| NanowebError::Http("Invalid UTF-8 in chunk size".into()))?;
                let chunk_size = usize::from_str_radix(size_str.trim(), 16)
                    .map_err(|_| NanowebError::Http(format!("Invalid chunk size: {}", size_str)))?;
                pos += line_end + 2;
                if chunk_size == 0 {
                    // consume trailing CRLF
                    if pos + 2 <= buf.len() && &buf[pos..pos + 2] == b"\r\n" {
                        pos += 2;
                    }
                    break;
                }
                if pos + chunk_size + 2 > buf.len() {
                    return Err(NanowebError::Http("Incomplete chunk data".into()));
                }
                body.extend_from_slice(&buf[pos..pos + chunk_size]);
                pos += chunk_size + 2; // skip CRLF after chunk data
            }
            Ok((body, pos))
        }
        BodyEncoding::None => Ok((Vec::new(), start)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_get() {
        let req = b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (parsed, consumed) = parse_request(req).unwrap();
        assert_eq!(parsed.method, Method::Get);
        assert_eq!(parsed.path, "/hello");
        assert_eq!(parsed.version, "HTTP/1.1");
        assert_eq!(parsed.headers.get("host"), Some("localhost"));
        assert_eq!(consumed, req.len());
    }

    #[test]
    fn parse_post_with_body() {
        let req = b"POST /submit HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n{\"key\":\"val\"}";
        let (parsed, _) = parse_request(req).unwrap();
        assert_eq!(parsed.method, Method::Post);
        assert_eq!(parsed.path, "/submit");
        assert_eq!(parsed.body, b"{\"key\":\"val\"}");
    }

    #[test]
    fn parse_chunked_body() {
        let req = b"POST /chunk HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nHello\r\n0\r\n\r\n";
        let (parsed, _) = parse_request(req).unwrap();
        assert_eq!(parsed.body, b"Hello");
    }

    #[test]
    fn parse_query_string() {
        let req = b"GET /search?q=rust&page=2 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (parsed, _) = parse_request(req).unwrap();
        assert_eq!(parsed.path, "/search");
        assert_eq!(parsed.query.get("q"), Some(&"rust".to_string()));
        assert_eq!(parsed.query.get("page"), Some(&"2".to_string()));
    }

    #[test]
    fn reject_bad_method() {
        let req = b"FOO /bar HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert!(parse_request(req).is_err());
    }

    #[test]
    fn reject_bad_version() {
        let req = b"GET /bar HTTP/2.0\r\nHost: localhost\r\n\r\n";
        assert!(parse_request(req).is_err());
    }

    #[test]
    fn reject_incomplete_body() {
        let req = b"POST /submit HTTP/1.1\r\nContent-Length: 100\r\n\r\nshort";
        assert!(parse_request(req).is_err());
    }
}

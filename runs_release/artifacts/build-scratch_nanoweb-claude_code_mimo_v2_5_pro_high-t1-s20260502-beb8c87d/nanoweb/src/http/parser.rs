use super::types::{Method, Request};
use crate::error::NanowebError;
use std::collections::HashMap;

const MAX_HEADER_SIZE: usize = 8192;
const MAX_BODY_SIZE: usize = 10 * 1024 * 1024; // 10 MB

pub fn parse_request(
    data: &[u8],
    request_id: String,
    remote_addr: Option<String>,
) -> Result<(Request, usize), NanowebError> {
    // Find the end of headers
    let header_end = find_header_end(data).ok_or_else(|| {
        if data.len() >= MAX_HEADER_SIZE {
            NanowebError::Http("Headers too large".to_string())
        } else {
            NanowebError::Http("Incomplete request".to_string())
        }
    })?;

    let header_bytes = &data[..header_end];
    let header_str = std::str::from_utf8(header_bytes)
        .map_err(|_| NanowebError::Http("Invalid UTF-8 in headers".to_string()))?;

    let mut lines = header_str.split("\r\n");

    // Parse request line
    let request_line = lines
        .next()
        .ok_or_else(|| NanowebError::Http("Empty request".to_string()))?;
    let (method, target, version) = parse_request_line(request_line)?;

    // Parse headers
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = parse_header_line(line) {
            headers.insert(name, value);
        }
    }

    // Parse path and query from target
    let (path, query) = parse_target(&target);

    // Determine body length
    let body_start = header_end + 4; // skip \r\n\r\n
    let chunked = headers
        .get("transfer-encoding")
        .map(|v| v.to_lowercase().contains("chunked"))
        .unwrap_or(false);

    let body = if chunked {
        parse_chunked_body(&data[body_start..])?
    } else if let Some(cl) = headers.get("content-length") {
        let len: usize = cl
            .parse()
            .map_err(|_| NanowebError::Http("Invalid Content-Length".to_string()))?;
        if len > MAX_BODY_SIZE {
            return Err(NanowebError::Http("Body too large".to_string()));
        }
        let body_end = body_start + len;
        if data.len() < body_end {
            return Err(NanowebError::Http("Incomplete body".to_string()));
        }
        data[body_start..body_end].to_vec()
    } else {
        Vec::new()
    };

    let total_consumed = if chunked {
        // For chunked, we need to figure out where the chunked body ends
        // This is already handled inside parse_chunked_body but we need the offset
        body_start + find_chunked_end(&data[body_start..]).unwrap_or(0)
    } else if headers.contains_key("content-length") {
        let len: usize = headers["content-length"].parse().unwrap_or(0);
        body_start + len
    } else {
        body_start
    };

    Ok((
        Request {
            method,
            target,
            path,
            query,
            version,
            headers,
            params: HashMap::new(),
            body,
            request_id,
            remote_addr,
        },
        total_consumed,
    ))
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if &data[i..i + 4] == b"\r\n\r\n" {
            return Some(i);
        }
    }
    None
}

fn parse_request_line(line: &str) -> Result<(Method, String, String), NanowebError> {
    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    if parts.len() != 3 {
        return Err(NanowebError::Http("Malformed request line".to_string()));
    }

    let method: Method = parts[0]
        .parse()
        .map_err(|_| NanowebError::Http(format!("Unknown method: {}", parts[0])))?;

    let target = parts[1].to_string();
    let version = parts[2].to_string();

    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(NanowebError::Http(format!(
            "Unsupported HTTP version: {version}"
        )));
    }

    Ok((method, target, version))
}

fn parse_header_line(line: &str) -> Option<(String, String)> {
    let colon_pos = line.find(':')?;
    let name = line[..colon_pos].trim().to_lowercase();
    let value = line[colon_pos + 1..].trim().to_string();
    Some((name, value))
}

fn parse_target(target: &str) -> (String, HashMap<String, String>) {
    if let Some(qpos) = target.find('?') {
        let path = percent_decode(&target[..qpos]);
        let query_str = &target[qpos + 1..];
        let query = parse_query_string(query_str);
        (path, query)
    } else {
        (percent_decode(target), HashMap::new())
    }
}

fn parse_query_string(s: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in s.split('&') {
        if pair.is_empty() {
            continue;
        }
        if let Some(eq_pos) = pair.find('=') {
            let key = percent_decode(&pair[..eq_pos]);
            let value = percent_decode(&pair[eq_pos + 1..]);
            map.insert(key, value);
        } else {
            map.insert(percent_decode(pair), String::new());
        }
    }
    map
}

fn percent_decode(s: &str) -> String {
    let mut result = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                result.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            result.push(b' ');
        } else {
            result.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8(result).unwrap_or_default()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_chunked_body(data: &[u8]) -> Result<Vec<u8>, NanowebError> {
    let mut result = Vec::new();
    let mut pos = 0;

    loop {
        // Find chunk size line
        let line_end = find_crlf(&data[pos..])
            .ok_or_else(|| NanowebError::Http("Invalid chunked encoding".to_string()))?;
        let size_str = std::str::from_utf8(&data[pos..pos + line_end])
            .map_err(|_| NanowebError::Http("Invalid chunk size".to_string()))?;
        let size_str = size_str.split(';').next().unwrap_or(size_str).trim();
        let chunk_size = usize::from_str_radix(size_str, 16)
            .map_err(|_| NanowebError::Http(format!("Invalid chunk size: {size_str}")))?;

        pos += line_end + 2; // skip \r\n

        if chunk_size == 0 {
            break;
        }

        if pos + chunk_size > data.len() {
            return Err(NanowebError::Http("Incomplete chunked body".to_string()));
        }

        result.extend_from_slice(&data[pos..pos + chunk_size]);
        pos += chunk_size + 2; // skip chunk data + \r\n
    }

    Ok(result)
}

fn find_chunked_end(data: &[u8]) -> Option<usize> {
    let mut pos = 0;
    loop {
        let line_end = find_crlf(&data[pos..])?;
        let size_str = std::str::from_utf8(&data[pos..pos + line_end]).ok()?;
        let size_str = size_str.split(';').next().unwrap_or(size_str).trim();
        let chunk_size = usize::from_str_radix(size_str, 16).ok()?;

        pos += line_end + 2;

        if chunk_size == 0 {
            return Some(pos + 2); // skip trailing \r\n
        }

        pos += chunk_size + 2;
    }
}

fn find_crlf(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(1) {
        if data[i] == b'\r' && data[i + 1] == b'\n' {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_get() {
        let raw = b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (req, _) = parse_request(raw, "test-1".into(), None).unwrap();
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.path, "/hello");
        assert_eq!(req.version, "HTTP/1.1");
        assert_eq!(req.header("host").unwrap(), "localhost");
    }

    #[test]
    fn test_parse_post_with_body() {
        let raw = b"POST /data HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nhello";
        let (req, _) = parse_request(raw, "test-2".into(), None).unwrap();
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.body, b"hello");
    }

    #[test]
    fn test_parse_query_string() {
        let raw = b"GET /search?q=hello+world&lang=en HTTP/1.1\r\nHost: x\r\n\r\n";
        let (req, _) = parse_request(raw, "test-3".into(), None).unwrap();
        assert_eq!(req.path, "/search");
        assert_eq!(req.query_param("q").unwrap(), "hello world");
        assert_eq!(req.query_param("lang").unwrap(), "en");
    }

    #[test]
    fn test_parse_chunked_body() {
        let raw = b"POST /chunked HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let (req, _) = parse_request(raw, "test-4".into(), None).unwrap();
        assert_eq!(req.body, b"hello world");
    }

    #[test]
    fn test_reject_bad_method() {
        let raw = b"INVALID / HTTP/1.1\r\nHost: x\r\n\r\n";
        assert!(parse_request(raw, "test-5".into(), None).is_err());
    }

    #[test]
    fn test_parse_with_percent_encoding() {
        let raw = b"GET /path%20with%20spaces HTTP/1.1\r\nHost: x\r\n\r\n";
        let (req, _) = parse_request(raw, "test-6".into(), None).unwrap();
        assert_eq!(req.path, "/path with spaces");
    }

    #[test]
    fn test_parse_header_case_insensitive() {
        let raw = b"GET / HTTP/1.1\r\nContent-Type: text/html\r\n\r\n";
        let (req, _) = parse_request(raw, "test-7".into(), None).unwrap();
        assert_eq!(req.header("content-type").unwrap(), "text/html");
        assert_eq!(req.header("Content-Type").unwrap(), "text/html");
    }
}

use crate::error::NanowebError;
use crate::http::types::{HttpVersion, Method, Request};
use std::collections::HashMap;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

const MAX_HEADER_SIZE: usize = 8192;
const MAX_BODY_SIZE: usize = 32 * 1024 * 1024; // 32 MB

pub struct RawParsed {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: Option<String>,
    pub version: HttpVersion,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub async fn read_request<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<RawParsed, NanowebError> {
    // Read request line
    let mut request_line = String::new();
    let n = reader
        .read_line(&mut request_line)
        .await
        .map_err(NanowebError::Io)?;
    if n == 0 {
        return Err(NanowebError::ConnectionClosed);
    }
    if n > MAX_HEADER_SIZE {
        return Err(NanowebError::BadRequest("Request line too long".into()));
    }

    let request_line = request_line.trim_end_matches(|c| c == '\r' || c == '\n');
    let (method, target, version) = parse_request_line(request_line)?;

    let (path, query) = split_target(&target);

    // Read headers
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut total_header_size = n;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(NanowebError::Io)?;
        if n == 0 {
            return Err(NanowebError::Parse("Unexpected EOF in headers".into()));
        }
        total_header_size += n;
        if total_header_size > MAX_HEADER_SIZE {
            return Err(NanowebError::BadRequest("Headers too large".into()));
        }

        let trimmed = line.trim_end_matches(|c| c == '\r' || c == '\n');
        if trimmed.is_empty() {
            break;
        }

        if let Some(colon) = trimmed.find(':') {
            let name = trimmed[..colon].trim().to_lowercase();
            let value = trimmed[colon + 1..].trim().to_string();
            if name.is_empty() {
                return Err(NanowebError::BadRequest("Empty header name".into()));
            }
            headers.push((name, value));
        } else {
            return Err(NanowebError::BadRequest(format!(
                "Malformed header: {}",
                trimmed
            )));
        }
    }

    // Determine body reading strategy
    let body = read_body(reader, &headers).await?;

    Ok(RawParsed {
        method,
        target,
        path,
        query,
        version,
        headers,
        body,
    })
}

async fn read_body<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    headers: &[(String, String)],
) -> Result<Vec<u8>, NanowebError> {
    let transfer_encoding = headers
        .iter()
        .find(|(k, _)| k == "transfer-encoding")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");

    if transfer_encoding.to_lowercase().contains("chunked") {
        return read_chunked_body(reader).await;
    }

    let content_length = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);

    if content_length == 0 {
        return Ok(Vec::new());
    }

    if content_length > MAX_BODY_SIZE {
        return Err(NanowebError::BadRequest("Body too large".into()));
    }

    let mut body = vec![0u8; content_length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(NanowebError::Io)?;
    Ok(body)
}

async fn read_chunked_body<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Vec<u8>, NanowebError> {
    let mut body = Vec::new();

    loop {
        let mut size_line = String::new();
        reader
            .read_line(&mut size_line)
            .await
            .map_err(NanowebError::Io)?;
        let size_str = size_line.trim().split(';').next().unwrap_or("").trim();
        let chunk_size = usize::from_str_radix(size_str, 16)
            .map_err(|_| NanowebError::Parse(format!("Invalid chunk size: {}", size_str)))?;

        if chunk_size == 0 {
            // Read trailing CRLF
            let mut trailing = String::new();
            reader
                .read_line(&mut trailing)
                .await
                .map_err(NanowebError::Io)?;
            break;
        }

        if body.len() + chunk_size > MAX_BODY_SIZE {
            return Err(NanowebError::BadRequest("Chunked body too large".into()));
        }

        let mut chunk = vec![0u8; chunk_size];
        reader
            .read_exact(&mut chunk)
            .await
            .map_err(NanowebError::Io)?;
        body.extend_from_slice(&chunk);

        // Read trailing CRLF after chunk
        let mut crlf = [0u8; 2];
        reader
            .read_exact(&mut crlf)
            .await
            .map_err(NanowebError::Io)?;
        if crlf != [b'\r', b'\n'] {
            return Err(NanowebError::Parse("Missing CRLF after chunk data".into()));
        }
    }

    Ok(body)
}

fn parse_request_line(line: &str) -> Result<(Method, String, HttpVersion), NanowebError> {
    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    if parts.len() != 3 {
        return Err(NanowebError::BadRequest(format!(
            "Invalid request line: {:?}",
            line
        )));
    }

    let method = Method::from_str(parts[0]);
    let target = parts[1].to_string();
    let version = match parts[2].trim() {
        "HTTP/1.0" => HttpVersion::Http10,
        "HTTP/1.1" => HttpVersion::Http11,
        v => {
            return Err(NanowebError::BadRequest(format!(
                "Unsupported HTTP version: {}",
                v
            )))
        }
    };

    Ok((method, target, version))
}

fn split_target(target: &str) -> (String, Option<String>) {
    if let Some(q_pos) = target.find('?') {
        let path = target[..q_pos].to_string();
        let query = target[q_pos + 1..].to_string();
        (path, if query.is_empty() { None } else { Some(query) })
    } else {
        (target.to_string(), None)
    }
}

pub fn into_request(parsed: RawParsed, remote_addr: String, request_id: String) -> Request {
    Request {
        method: parsed.method,
        target: parsed.target,
        path: parsed.path,
        query: parsed.query,
        version: parsed.version,
        headers: parsed.headers,
        body: parsed.body,
        params: HashMap::new(),
        remote_addr,
        request_id,
    }
}

pub fn decode_chunked(data: &[u8]) -> Result<Vec<u8>, NanowebError> {
    let mut result = Vec::new();
    let mut pos = 0;

    loop {
        let line_end = data[pos..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| NanowebError::Parse("Invalid chunked encoding".into()))?;

        let size_str = std::str::from_utf8(&data[pos..pos + line_end])
            .map_err(|_| NanowebError::Parse("Invalid chunk size encoding".into()))?;
        let size_str = size_str.split(';').next().unwrap_or("").trim();

        let chunk_size = usize::from_str_radix(size_str, 16)
            .map_err(|_| NanowebError::Parse(format!("Invalid chunk size: {}", size_str)))?;

        pos += line_end + 2;

        if chunk_size == 0 {
            break;
        }

        if pos + chunk_size > data.len() {
            return Err(NanowebError::Parse("Chunk extends beyond buffer".into()));
        }

        result.extend_from_slice(&data[pos..pos + chunk_size]);
        pos += chunk_size + 2;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn test_parse_simple_get() {
        let raw = b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let mut reader = BufReader::new(&raw[..]);
        let parsed = read_request(&mut reader).await.unwrap();
        assert_eq!(parsed.method, Method::Get);
        assert_eq!(parsed.path, "/hello");
        assert!(parsed.query.is_none());
        assert!(parsed.body.is_empty());
    }

    #[tokio::test]
    async fn test_parse_post_with_body() {
        let raw = b"POST /echo HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let mut reader = BufReader::new(&raw[..]);
        let parsed = read_request(&mut reader).await.unwrap();
        assert_eq!(parsed.method, Method::Post);
        assert_eq!(parsed.body, b"hello");
    }

    #[tokio::test]
    async fn test_parse_chunked_body() {
        let raw =
            b"POST /data HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let mut reader = BufReader::new(&raw[..]);
        let parsed = read_request(&mut reader).await.unwrap();
        assert_eq!(parsed.body, b"hello");
    }

    #[tokio::test]
    async fn test_parse_query_string() {
        let raw = b"GET /search?q=rust&page=1 HTTP/1.1\r\n\r\n";
        let mut reader = BufReader::new(&raw[..]);
        let parsed = read_request(&mut reader).await.unwrap();
        assert_eq!(parsed.path, "/search");
        assert_eq!(parsed.query, Some("q=rust&page=1".into()));
    }

    #[tokio::test]
    async fn test_invalid_request_line() {
        let raw = b"INVALID\r\n\r\n";
        let mut reader = BufReader::new(&raw[..]);
        let result = read_request(&mut reader).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_malformed_header() {
        let raw = b"GET / HTTP/1.1\r\nBadHeader\r\n\r\n";
        let mut reader = BufReader::new(&raw[..]);
        let result = read_request(&mut reader).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_chunked() {
        let data = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let result = decode_chunked(data).unwrap();
        assert_eq!(result, b"hello world");
    }
}

//! Byte-level HTTP/1.1 parser. Has no dependency on router/middleware/body.

use super::types::{Headers, Method, Version};
use crate::error::{NanowebError, Result};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::net::TcpStream;

const MAX_LINE: usize = 8 * 1024;
const MAX_HEADERS: usize = 100;
pub const MAX_BODY: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ParsedRequestHead {
    pub method: Method,
    pub target: String,
    pub version: Version,
    pub headers: Headers,
}

pub fn parse_request_line(line: &str) -> Result<(Method, String, Version)> {
    let mut parts = line.splitn(3, ' ');
    let m = parts
        .next()
        .ok_or_else(|| NanowebError::BadRequest("missing method".into()))?;
    let target = parts
        .next()
        .ok_or_else(|| NanowebError::BadRequest("missing target".into()))?;
    let ver = parts
        .next()
        .ok_or_else(|| NanowebError::BadRequest("missing version".into()))?;
    let method = Method::parse(m)
        .ok_or_else(|| NanowebError::BadRequest(format!("unknown method {}", m)))?;
    let version = match ver.trim_end() {
        "HTTP/1.0" => Version::Http10,
        "HTTP/1.1" => Version::Http11,
        other => return Err(NanowebError::BadRequest(format!("bad version {}", other))),
    };
    if target.is_empty() {
        return Err(NanowebError::BadRequest("empty target".into()));
    }
    Ok((method, target.to_string(), version))
}

pub fn parse_header_line(line: &str) -> Result<(String, String)> {
    let idx = line
        .find(':')
        .ok_or_else(|| NanowebError::BadRequest("bad header".into()))?;
    let (name, rest) = line.split_at(idx);
    if name.is_empty() || name.chars().any(|c| c.is_whitespace()) {
        return Err(NanowebError::BadRequest("bad header name".into()));
    }
    let value = rest[1..].trim();
    Ok((name.to_string(), value.to_string()))
}

pub async fn read_request_head<R>(reader: &mut BufReader<R>) -> Result<ParsedRequestHead>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Err(NanowebError::BadRequest("connection closed".into()));
    }
    if n > MAX_LINE {
        return Err(NanowebError::BadRequest("request line too long".into()));
    }
    let line_trim = line.trim_end_matches(['\r', '\n']);
    let (method, target, version) = parse_request_line(line_trim)?;

    let mut headers = Headers::new();
    let mut count = 0usize;
    loop {
        let mut hl = String::new();
        let n = reader.read_line(&mut hl).await?;
        if n == 0 {
            return Err(NanowebError::BadRequest("eof in headers".into()));
        }
        if n > MAX_LINE {
            return Err(NanowebError::BadRequest("header line too long".into()));
        }
        let trimmed = hl.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if count >= MAX_HEADERS {
            return Err(NanowebError::BadRequest("too many headers".into()));
        }
        let (name, value) = parse_header_line(trimmed)?;
        headers.append(name, value);
        count += 1;
    }

    Ok(ParsedRequestHead {
        method,
        target,
        version,
        headers,
    })
}

pub async fn read_body<R>(reader: &mut BufReader<R>, headers: &Headers) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    if let Some(te) = headers.get("Transfer-Encoding") {
        if te.eq_ignore_ascii_case("chunked") {
            return read_chunked(reader).await;
        } else {
            return Err(NanowebError::BadRequest(format!(
                "unsupported transfer-encoding {}",
                te
            )));
        }
    }
    if let Some(cl) = headers.get("Content-Length") {
        let n: usize = cl
            .trim()
            .parse()
            .map_err(|_| NanowebError::BadRequest("bad content-length".into()))?;
        if n > MAX_BODY {
            return Err(NanowebError::BadRequest("body too large".into()));
        }
        let mut buf = vec![0u8; n];
        reader.read_exact(&mut buf).await?;
        return Ok(buf);
    }
    Ok(Vec::new())
}

async fn read_chunked<R>(reader: &mut BufReader<R>) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut out = Vec::new();
    loop {
        let mut size_line = String::new();
        let n = reader.read_line(&mut size_line).await?;
        if n == 0 {
            return Err(NanowebError::BadRequest("eof in chunk size".into()));
        }
        let size_str = size_line.trim().split(';').next().unwrap_or("");
        let size = usize::from_str_radix(size_str, 16)
            .map_err(|_| NanowebError::BadRequest("bad chunk size".into()))?;
        if size == 0 {
            // trailing CRLF (and optional trailers ignored)
            loop {
                let mut t = String::new();
                let m = reader.read_line(&mut t).await?;
                if m == 0 {
                    break;
                }
                if t.trim().is_empty() {
                    break;
                }
            }
            return Ok(out);
        }
        if out.len() + size > MAX_BODY {
            return Err(NanowebError::BadRequest("body too large".into()));
        }
        let mut buf = vec![0u8; size];
        reader.read_exact(&mut buf).await?;
        out.extend_from_slice(&buf);
        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf).await?;
        if crlf != *b"\r\n" {
            return Err(NanowebError::BadRequest("missing chunk crlf".into()));
        }
    }
}

pub fn split_target(target: &str) -> (String, String) {
    if let Some(idx) = target.find('?') {
        (target[..idx].to_string(), target[idx + 1..].to_string())
    } else {
        (target.to_string(), String::new())
    }
}

#[allow(unused_imports)]
pub trait AsyncReadStream: tokio::io::AsyncRead + Unpin {}
impl AsyncReadStream for TcpStream {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_line() {
        let (m, t, v) = parse_request_line("GET /foo HTTP/1.1").unwrap();
        assert_eq!(m, Method::Get);
        assert_eq!(t, "/foo");
        assert_eq!(v, Version::Http11);
    }

    #[test]
    fn parse_line_bad() {
        assert!(parse_request_line("WHAT /").is_err());
        assert!(parse_request_line("GET").is_err());
        assert!(parse_request_line("GET / HTTP/9").is_err());
    }

    #[test]
    fn parse_header() {
        let (n, v) = parse_header_line("Host: example.com").unwrap();
        assert_eq!(n, "Host");
        assert_eq!(v, "example.com");
    }

    #[test]
    fn split_target_query() {
        let (p, q) = split_target("/x?y=1");
        assert_eq!(p, "/x");
        assert_eq!(q, "y=1");
    }

    #[tokio::test]
    async fn parse_full_request() {
        let raw = b"GET /a HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhello";
        let cursor = std::io::Cursor::new(raw.to_vec());
        let mut r = BufReader::new(cursor);
        let head = read_request_head(&mut r).await.unwrap();
        assert_eq!(head.method, Method::Get);
        assert_eq!(head.target, "/a");
        let body = read_body(&mut r, &head.headers).await.unwrap();
        assert_eq!(body, b"hello");
    }

    #[tokio::test]
    async fn parse_chunked() {
        let raw = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let cursor = std::io::Cursor::new(raw.to_vec());
        let mut r = BufReader::new(cursor);
        let head = read_request_head(&mut r).await.unwrap();
        let body = read_body(&mut r, &head.headers).await.unwrap();
        assert_eq!(body, b"hello world");
    }
}

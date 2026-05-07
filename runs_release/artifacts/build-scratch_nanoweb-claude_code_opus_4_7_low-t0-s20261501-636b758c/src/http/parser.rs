//! Byte-level HTTP/1.1 request parser.
//!
//! Independent of routing/middleware/body modules so it can be exercised in
//! isolation.

use super::{Headers, Method, Request};
use crate::error::NanowebError;
use std::str::FromStr;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::net::TcpStream;

const MAX_LINE: usize = 8 * 1024;
const MAX_HEADERS_BYTES: usize = 64 * 1024;
const MAX_BODY: usize = 16 * 1024 * 1024;

pub async fn read_request<R>(stream: &mut BufReader<R>) -> Result<Option<Request>, NanowebError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut request_line = Vec::new();
    let n = stream
        .read_until(b'\n', &mut request_line)
        .await
        .map_err(NanowebError::Io)?;
    if n == 0 {
        return Ok(None);
    }
    if request_line.len() > MAX_LINE {
        return Err(NanowebError::Parse("request line too long".into()));
    }
    let line = strip_crlf(&request_line);
    let line_str = std::str::from_utf8(line)
        .map_err(|_| NanowebError::Parse("non-utf8 request line".into()))?;
    let mut parts = line_str.splitn(3, ' ');
    let method_s = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing method".into()))?;
    let target = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing target".into()))?;
    let version = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing version".into()))?;
    if !version.starts_with("HTTP/1.") {
        return Err(NanowebError::Parse("unsupported version".into()));
    }
    let method = Method::from_str(method_s)
        .map_err(|_| NanowebError::Parse(format!("bad method: {}", method_s)))?;

    let mut headers = Headers::new();
    let mut total = 0usize;
    loop {
        let mut buf = Vec::new();
        let n = stream
            .read_until(b'\n', &mut buf)
            .await
            .map_err(NanowebError::Io)?;
        if n == 0 {
            return Err(NanowebError::Parse("eof in headers".into()));
        }
        total += n;
        if total > MAX_HEADERS_BYTES {
            return Err(NanowebError::Parse("headers too large".into()));
        }
        let line = strip_crlf(&buf);
        if line.is_empty() {
            break;
        }
        let s =
            std::str::from_utf8(line).map_err(|_| NanowebError::Parse("non-utf8 header".into()))?;
        let (k, v) = s
            .split_once(':')
            .ok_or_else(|| NanowebError::Parse("bad header".into()))?;
        if k.is_empty() || k.contains(' ') {
            return Err(NanowebError::Parse("bad header name".into()));
        }
        headers.append(k.trim(), v.trim());
    }

    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.to_string(), String::new()),
    };

    let mut body = Vec::new();
    let te = headers
        .get("transfer-encoding")
        .map(|s| s.to_ascii_lowercase());
    let cl = headers
        .get("content-length")
        .and_then(|s| s.trim().parse::<usize>().ok());

    if let Some(te) = te.as_deref() {
        if te == "chunked" {
            read_chunked(stream, &mut body).await?;
        } else if te != "identity" {
            return Err(NanowebError::Parse("unsupported transfer-encoding".into()));
        }
    } else if let Some(len) = cl {
        if len > MAX_BODY {
            return Err(NanowebError::Parse("body too large".into()));
        }
        body.resize(len, 0);
        if len > 0 {
            stream
                .read_exact(&mut body)
                .await
                .map_err(NanowebError::Io)?;
        }
    }

    Ok(Some(Request {
        method,
        target: target.to_string(),
        path,
        query,
        version: version.to_string(),
        headers,
        body,
        params: Default::default(),
        remote_addr: None,
        request_id: String::new(),
        extensions: Default::default(),
    }))
}

async fn read_chunked<R>(stream: &mut BufReader<R>, out: &mut Vec<u8>) -> Result<(), NanowebError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    loop {
        let mut size_line = Vec::new();
        stream
            .read_until(b'\n', &mut size_line)
            .await
            .map_err(NanowebError::Io)?;
        let s = strip_crlf(&size_line);
        let s = std::str::from_utf8(s).map_err(|_| NanowebError::Parse("bad chunk size".into()))?;
        let s = s.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(s, 16)
            .map_err(|_| NanowebError::Parse("bad chunk size".into()))?;
        if size == 0 {
            // discard trailers up to blank line
            loop {
                let mut t = Vec::new();
                let n = stream
                    .read_until(b'\n', &mut t)
                    .await
                    .map_err(NanowebError::Io)?;
                if n == 0 || strip_crlf(&t).is_empty() {
                    break;
                }
            }
            return Ok(());
        }
        if out.len() + size > MAX_BODY {
            return Err(NanowebError::Parse("body too large".into()));
        }
        let start = out.len();
        out.resize(start + size, 0);
        stream
            .read_exact(&mut out[start..])
            .await
            .map_err(NanowebError::Io)?;
        // CRLF after chunk
        let mut crlf = [0u8; 2];
        stream
            .read_exact(&mut crlf)
            .await
            .map_err(NanowebError::Io)?;
        if &crlf != b"\r\n" {
            return Err(NanowebError::Parse("bad chunk terminator".into()));
        }
    }
}

fn strip_crlf(buf: &[u8]) -> &[u8] {
    let mut end = buf.len();
    while end > 0 && (buf[end - 1] == b'\n' || buf[end - 1] == b'\r') {
        end -= 1;
    }
    &buf[..end]
}

pub async fn read_request_tcp(
    stream: &mut BufReader<&mut TcpStream>,
) -> Result<Option<Request>, NanowebError> {
    read_request(stream).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn parses_simple_get() {
        let raw = b"GET /hello?x=1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let cursor = Cursor::new(raw.to_vec());
        let mut br = BufReader::new(cursor);
        let req = read_request(&mut br).await.unwrap().unwrap();
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.path, "/hello");
        assert_eq!(req.query, "x=1");
        assert_eq!(req.headers.get("host"), Some("localhost"));
    }

    #[tokio::test]
    async fn parses_content_length_body() {
        let raw = b"POST /a HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let cursor = Cursor::new(raw.to_vec());
        let mut br = BufReader::new(cursor);
        let req = read_request(&mut br).await.unwrap().unwrap();
        assert_eq!(req.body, b"hello");
    }

    #[tokio::test]
    async fn parses_chunked() {
        let raw = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let cursor = Cursor::new(raw.to_vec());
        let mut br = BufReader::new(cursor);
        let req = read_request(&mut br).await.unwrap().unwrap();
        assert_eq!(req.body, b"hello world");
    }

    #[tokio::test]
    async fn rejects_bad_method() {
        let raw = b"BOGUS / HTTP/1.1\r\n\r\n";
        let cursor = Cursor::new(raw.to_vec());
        let mut br = BufReader::new(cursor);
        assert!(read_request(&mut br).await.is_err());
    }

    #[tokio::test]
    async fn rejects_bad_version() {
        let raw = b"GET / HTTP/9.9\r\n\r\n";
        let cursor = Cursor::new(raw.to_vec());
        let mut br = BufReader::new(cursor);
        assert!(read_request(&mut br).await.is_err());
    }
}

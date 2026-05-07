use super::{Method, Request};
use crate::error::{NanowebError, Result};
use rand::{Rng, distributions::Alphanumeric};
use std::collections::HashMap;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

const MAX_LINE: usize = 8192;
const MAX_BODY: usize = 16 * 1024 * 1024;

pub async fn parse_request<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Request> {
    let mut request_line = String::new();
    let n = reader.read_line(&mut request_line).await?;
    if n == 0 {
        return Err(NanowebError::Parse("empty request".into()));
    }
    if request_line.len() > MAX_LINE || !request_line.ends_with("\r\n") {
        return Err(NanowebError::Parse("invalid request line".into()));
    }
    let parts: Vec<&str> = request_line.trim_end_matches("\r\n").split(' ').collect();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" || parts[1].is_empty() {
        return Err(NanowebError::Parse("malformed request line".into()));
    }

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(NanowebError::Parse("unexpected eof in headers".into()));
        }
        if line.len() > MAX_LINE {
            return Err(NanowebError::Parse("header line too long".into()));
        }
        if line == "\r\n" {
            break;
        }
        let (name, value) = line
            .trim_end_matches("\r\n")
            .split_once(':')
            .ok_or_else(|| NanowebError::Parse("malformed header".into()))?;
        if name.is_empty() || name.bytes().any(|b| b <= b' ' || b == b':') {
            return Err(NanowebError::Parse("invalid header name".into()));
        }
        headers.insert(name.to_ascii_lowercase(), value.trim().to_string());
    }

    let body = if headers.get("transfer-encoding").is_some_and(|v| {
        v.to_ascii_lowercase()
            .split(',')
            .any(|p| p.trim() == "chunked")
    }) {
        read_chunked(reader).await?
    } else if let Some(len) = headers.get("content-length") {
        let len: usize = len
            .parse()
            .map_err(|_| NanowebError::Parse("invalid content-length".into()))?;
        if len > MAX_BODY {
            return Err(NanowebError::Parse("body too large".into()));
        }
        let mut body = vec![0; len];
        reader.read_exact(&mut body).await?;
        body
    } else {
        Vec::new()
    };

    let target = parts[1].to_string();
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (target.clone(), None),
    };

    Ok(Request {
        method: Method::from(parts[0]),
        target,
        path,
        query,
        version: parts[2].to_string(),
        headers,
        body,
        peer_addr: None,
        request_id: rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(16)
            .map(char::from)
            .collect(),
        params: HashMap::new(),
    })
}

async fn read_chunked<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        reader.read_line(&mut size_line).await?;
        let size_hex = size_line
            .trim_end_matches("\r\n")
            .split(';')
            .next()
            .unwrap_or("");
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| NanowebError::Parse("invalid chunk size".into()))?;
        if body.len() + size > MAX_BODY {
            return Err(NanowebError::Parse("body too large".into()));
        }
        if size == 0 {
            loop {
                let mut trailer = String::new();
                reader.read_line(&mut trailer).await?;
                if trailer == "\r\n" {
                    return Ok(body);
                }
            }
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..]).await?;
        let mut crlf = [0_u8; 2];
        reader.read_exact(&mut crlf).await?;
        if crlf != *b"\r\n" {
            return Err(NanowebError::Parse("chunk missing crlf".into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn parses_content_length() {
        let input = b"POST /x?a=1 HTTP/1.1\r\nHost: a\r\nContent-Length: 5\r\n\r\nhello";
        let mut reader = BufReader::new(&input[..]);
        let req = parse_request(&mut reader).await.unwrap();
        assert_eq!(req.path, "/x");
        assert_eq!(req.body, b"hello");
    }

    #[tokio::test]
    async fn parses_chunked() {
        let input = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let mut reader = BufReader::new(&input[..]);
        let req = parse_request(&mut reader).await.unwrap();
        assert_eq!(req.body, b"hello");
    }
}

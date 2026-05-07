use super::{Method, Request};
use crate::error::{NanowebError, Result};
use std::collections::HashMap;
use tokio::io::{AsyncRead, AsyncReadExt};
use uuid::Uuid;

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

pub async fn read_request<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Request> {
    let mut buf = Vec::new();
    let header_end = loop {
        let mut byte = [0_u8; 1];
        let n = reader.read(&mut byte).await?;
        if n == 0 {
            return Err(NanowebError::Parse(
                "connection closed before headers".into(),
            ));
        }
        buf.push(byte[0]);
        if buf.len() > MAX_HEADER_BYTES {
            return Err(NanowebError::Parse("headers too large".into()));
        }
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
    };
    let head = std::str::from_utf8(&buf[..header_end])
        .map_err(|_| NanowebError::Parse("headers are not utf-8".into()))?;
    let mut lines = head.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| NanowebError::Parse("missing request line".into()))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing method".into()))?;
    let target = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing target".into()))?;
    let version = parts
        .next()
        .ok_or_else(|| NanowebError::Parse("missing version".into()))?;
    if parts.next().is_some() || version != "HTTP/1.1" {
        return Err(NanowebError::Parse("invalid request line".into()));
    }
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| NanowebError::Parse("malformed header".into()))?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    let already = buf[(header_end + 4)..].to_vec();
    let body = if header_eq(&headers, "transfer-encoding", "chunked") {
        read_chunked(reader, already).await?
    } else if let Some(len) = headers.get("content-length") {
        let len: usize = len
            .parse()
            .map_err(|_| NanowebError::Parse("invalid content-length".into()))?;
        if len > MAX_BODY_BYTES {
            return Err(NanowebError::Parse("body too large".into()));
        }
        read_fixed(reader, already, len).await?
    } else {
        Vec::new()
    };
    Ok(Request {
        method: Method::from(method),
        target: target.to_string(),
        version: version.to_string(),
        headers,
        body,
        params: HashMap::new(),
        client_ip: None,
        request_id: Uuid::new_v4().to_string(),
    })
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn header_eq(headers: &HashMap<String, String>, name: &str, value: &str) -> bool {
    headers
        .get(name)
        .is_some_and(|v| v.to_ascii_lowercase().split(',').any(|x| x.trim() == value))
}

async fn read_fixed<R: AsyncRead + Unpin>(
    reader: &mut R,
    mut body: Vec<u8>,
    len: usize,
) -> Result<Vec<u8>> {
    if body.len() > len {
        body.truncate(len);
    }
    let have = body.len();
    if have < len {
        body.resize(len, 0);
        reader.read_exact(&mut body[have..]).await?;
    }
    Ok(body)
}

async fn read_line<R: AsyncRead + Unpin>(reader: &mut R, stash: &mut Vec<u8>) -> Result<String> {
    loop {
        if let Some(pos) = stash.windows(2).position(|w| w == b"\r\n") {
            let line = stash.drain(..pos).collect::<Vec<_>>();
            stash.drain(..2);
            return String::from_utf8(line)
                .map_err(|_| NanowebError::Parse("chunk line is not utf-8".into()));
        }
        let mut b = [0_u8; 1];
        if reader.read(&mut b).await? == 0 {
            return Err(NanowebError::Parse("unexpected eof in chunked body".into()));
        }
        stash.push(b[0]);
    }
}

async fn read_chunked<R: AsyncRead + Unpin>(reader: &mut R, mut stash: Vec<u8>) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let line = read_line(reader, &mut stash).await?;
        let size_hex = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| NanowebError::Parse("invalid chunk size".into()))?;
        if size == 0 {
            let _ = read_line(reader, &mut stash).await?;
            return Ok(body);
        }
        if body.len() + size > MAX_BODY_BYTES {
            return Err(NanowebError::Parse("body too large".into()));
        }
        while stash.len() < size + 2 {
            let mut tmp = vec![0; size + 2 - stash.len()];
            reader.read_exact(&mut tmp).await?;
            stash.extend(tmp);
        }
        body.extend(stash.drain(..size));
        if stash.drain(..2).collect::<Vec<_>>() != b"\r\n" {
            return Err(NanowebError::Parse("missing chunk terminator".into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn parses_chunked() {
        let raw = b"POST /x HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        let mut c = std::io::Cursor::new(raw);
        let req = read_request(&mut c).await.unwrap();
        assert_eq!(req.body, b"Wikipedia");
    }
}

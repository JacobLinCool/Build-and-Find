use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

use crate::error::{NanowebError, Result};

use super::{HeaderMap, Method, Request, Version};

pub async fn read_request<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<Request>> {
    let mut request_line = String::new();
    let bytes = reader.read_line(&mut request_line).await?;
    if bytes == 0 {
        return Ok(None);
    }

    let request_line = request_line.trim_end_matches(['\r', '\n']);
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| NanowebError::Request("missing method".into()))?;
    let target = parts
        .next()
        .ok_or_else(|| NanowebError::Request("missing target".into()))?;
    let version = parts
        .next()
        .ok_or_else(|| NanowebError::Request("missing version".into()))?;
    if parts.next().is_some() {
        return Err(NanowebError::Request("malformed request line".into()));
    }

    let method: Method = method.parse()?;
    let version = match version {
        "HTTP/1.1" => Version::Http11,
        other => {
            return Err(NanowebError::Request(format!(
                "unsupported http version: {other}"
            )));
        }
    };

    let mut headers = HeaderMap::new();
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            return Err(NanowebError::Request("unexpected eof in headers".into()));
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| NanowebError::Request(format!("invalid header line: {line}")))?;
        headers.insert(name.trim(), value.trim().to_string());
    }

    let transfer_encoding = headers
        .get("transfer-encoding")
        .map(|v| v.to_ascii_lowercase());
    let content_length = headers.get("content-length").map(|v| v.to_string());
    if transfer_encoding.is_some() && content_length.is_some() {
        return Err(NanowebError::Request(
            "content-length and transfer-encoding cannot both be set".into(),
        ));
    }

    let body = if matches!(transfer_encoding.as_deref(), Some("chunked")) {
        read_chunked_body(reader).await?
    } else if let Some(content_length) = content_length {
        let len: usize = content_length
            .parse()
            .map_err(|_| NanowebError::Request("invalid content-length".into()))?;
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body).await?;
        body
    } else {
        Vec::new()
    };

    let (path, query) = split_target(target);
    Ok(Some(Request {
        method,
        target: target.to_string(),
        path,
        query,
        version,
        headers,
        body,
        params: Default::default(),
        remote_addr: None,
        request_id: String::new(),
        session: None,
    }))
}

fn split_target(target: &str) -> (String, Option<String>) {
    if let Some((path, query)) = target.split_once('?') {
        (path.to_string(), Some(query.to_string()))
    } else {
        (target.to_string(), None)
    }
}

async fn read_chunked_body<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        let bytes = reader.read_line(&mut size_line).await?;
        if bytes == 0 {
            return Err(NanowebError::Request(
                "unexpected eof in chunked body".into(),
            ));
        }
        let size_line = size_line.trim_end_matches(['\r', '\n']);
        let size_part = size_line.split(';').next().unwrap_or(size_line);
        let size = usize::from_str_radix(size_part.trim(), 16)
            .map_err(|_| NanowebError::Request("invalid chunk size".into()))?;
        if size == 0 {
            loop {
                let mut trailer = String::new();
                let bytes = reader.read_line(&mut trailer).await?;
                if bytes == 0 {
                    return Err(NanowebError::Request(
                        "unexpected eof in chunk trailers".into(),
                    ));
                }
                if trailer.trim_end_matches(['\r', '\n']).is_empty() {
                    break;
                }
            }
            break;
        }

        let mut chunk = vec![0u8; size];
        reader.read_exact(&mut chunk).await?;
        body.extend_from_slice(&chunk);

        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf).await?;
        if &crlf != b"\r\n" {
            return Err(NanowebError::Request("invalid chunk terminator".into()));
        }
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn parses_content_length() {
        let data = b"POST /submit HTTP/1.1\r\ncontent-length: 5\r\n\r\nhello";
        let mut reader = BufReader::new(&data[..]);
        let req = read_request(&mut reader).await.unwrap().unwrap();
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.body, b"hello");
    }

    #[tokio::test]
    async fn parses_chunked() {
        let data =
            b"POST /submit HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let mut reader = BufReader::new(&data[..]);
        let req = read_request(&mut reader).await.unwrap().unwrap();
        assert_eq!(req.body, b"hello");
    }
}

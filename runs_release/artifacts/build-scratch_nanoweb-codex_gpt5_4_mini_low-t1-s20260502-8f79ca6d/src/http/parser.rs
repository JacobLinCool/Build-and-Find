use crate::error::NanowebError;
use crate::http::{Method, Request, Version};
use std::collections::HashMap;

pub fn parse_request(
    buf: &[u8],
    remote_addr: Option<std::net::SocketAddr>,
) -> Result<Request, NanowebError> {
    let header_end = find_header_end(buf).ok_or_else(|| "bad request".to_string())?;
    let head = &buf[..header_end];
    let body = &buf[header_end..];
    let head_text = std::str::from_utf8(head).map_err(|_| "bad request".to_string())?;
    let mut lines = head_text.split("\r\n");
    let request_line = lines.next().ok_or_else(|| "bad request".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or_else(|| "bad request".to_string())?;
    let target = parts.next().ok_or_else(|| "bad request".to_string())?;
    let version = parts.next().ok_or_else(|| "bad request".to_string())?;
    if parts.next().is_some() {
        return Err("bad request".into());
    }
    let version = match version {
        "HTTP/1.1" => Version::Http11,
        "HTTP/1.0" => Version::Http10,
        _ => return Err("bad request".into()),
    };
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (k, v) = line
            .split_once(':')
            .ok_or_else(|| "bad request".to_string())?;
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    let body = if let Some(te) = headers.get("transfer-encoding") {
        if te.eq_ignore_ascii_case("chunked") {
            parse_chunked(body)?
        } else {
            return Err("bad request".into());
        }
    } else if let Some(len) = headers.get("content-length") {
        let len: usize = len.parse().map_err(|_| "bad request".to_string())?;
        if body.len() < len {
            return Err("bad request".into());
        }
        body[..len].to_vec()
    } else {
        Vec::new()
    };
    let (path, query_params) = parse_target(target)?;
    Ok(Request {
        method: Method::from(method),
        target: path,
        version,
        headers,
        body,
        path_params: HashMap::new(),
        query_params,
        remote_addr,
        request_id: uuid::Uuid::new_v4().to_string(),
    })
}

fn parse_target(target: &str) -> Result<(String, HashMap<String, String>), NanowebError> {
    let mut query_params = HashMap::new();
    let (path, query) = if let Some((p, q)) = target.split_once('?') {
        (p, Some(q))
    } else {
        (target, None)
    };
    if let Some(q) = query {
        for (k, v) in url::form_urlencoded::parse(q.as_bytes()) {
            query_params.insert(k.into_owned(), v.into_owned());
        }
    }
    Ok((path.to_string(), query_params))
}

fn parse_chunked(mut body: &[u8]) -> Result<Vec<u8>, NanowebError> {
    let mut out = Vec::new();
    loop {
        let line_end = find_crlf(body).ok_or_else(|| "bad request".to_string())?;
        let size_line =
            std::str::from_utf8(&body[..line_end]).map_err(|_| "bad request".to_string())?;
        let size =
            usize::from_str_radix(size_line.trim(), 16).map_err(|_| "bad request".to_string())?;
        body = &body[line_end + 2..];
        if size == 0 {
            break;
        }
        if body.len() < size + 2 || &body[size..size + 2] != b"\r\n" {
            return Err("bad request".into());
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
    }
    Ok(out)
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_content_length() {
        let req = parse_request(
            b"GET /a?x=1 HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhello",
            None,
        )
        .unwrap();
        assert_eq!(req.target, "/a");
        assert_eq!(req.query_params.get("x").unwrap(), "1");
        assert_eq!(req.body, b"hello");
    }
    #[test]
    fn parses_chunked() {
        let req = parse_request(
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
            None,
        )
        .unwrap();
        assert_eq!(req.body, b"hello");
    }
}

//! Body parsers for JSON, urlencoded, and multipart/form-data.

use crate::http::Request;
use crate::NanowebError;
use serde::de::DeserializeOwned;
use std::collections::HashMap;

pub fn parse_body<T: DeserializeOwned>(req: &Request) -> Result<T, NanowebError> {
    let ctype = req
        .header("content-type")
        .unwrap_or("application/octet-stream")
        .to_string();
    let main = ctype
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match main.as_str() {
        "application/json" => Ok(serde_json::from_slice::<T>(&req.body)?),
        "application/x-www-form-urlencoded" => serde_urlencoded::from_bytes::<T>(&req.body)
            .map_err(|e| NanowebError::Body(e.to_string())),
        "multipart/form-data" => {
            let boundary = boundary_from(&ctype)
                .ok_or_else(|| NanowebError::Body("missing multipart boundary".into()))?;
            let parts = parse_multipart(&req.body, &boundary)?;
            let map: HashMap<String, String> = parts
                .into_iter()
                .filter_map(|p| {
                    let v = String::from_utf8(p.body).ok()?;
                    Some((p.name, v))
                })
                .collect();
            let json = serde_json::to_value(map).map_err(NanowebError::from)?;
            serde_json::from_value::<T>(json).map_err(NanowebError::from)
        }
        other => Err(NanowebError::Body(format!(
            "unsupported content-type: {}",
            other
        ))),
    }
}

#[derive(Debug, Clone)]
pub struct MultipartPart {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

fn boundary_from(ctype: &str) -> Option<String> {
    for part in ctype.split(';') {
        let part = part.trim();
        if let Some(b) = part.strip_prefix("boundary=") {
            let b = b.trim_matches('"');
            return Some(b.to_string());
        }
    }
    None
}

pub fn parse_multipart(body: &[u8], boundary: &str) -> Result<Vec<MultipartPart>, NanowebError> {
    let dash_boundary = format!("--{}", boundary);
    let close = format!("--{}--", boundary);
    let s = body;
    let mut parts = Vec::new();
    // Split by boundary
    let bdy = dash_boundary.as_bytes();
    let close_b = close.as_bytes();
    // Find first boundary
    let first = find_subsequence(s, bdy).ok_or_else(|| NanowebError::Body("no boundary".into()))?;
    let mut idx = first + bdy.len();
    loop {
        // Skip CRLF after boundary marker
        if idx + 2 <= s.len() && &s[idx..idx + 2] == b"\r\n" {
            idx += 2;
        } else if idx + 2 <= s.len() && &s[idx..idx + 2] == b"--" {
            // closing boundary
            return Ok(parts);
        } else {
            return Ok(parts);
        }
        let next = find_subsequence(&s[idx..], format!("\r\n--{}", boundary).as_bytes())
            .ok_or_else(|| NanowebError::Body("unterminated part".into()))?;
        let part_bytes = &s[idx..idx + next];
        // Headers and body separated by \r\n\r\n
        let header_end = find_subsequence(part_bytes, b"\r\n\r\n")
            .ok_or_else(|| NanowebError::Body("part missing headers".into()))?;
        let headers_str = std::str::from_utf8(&part_bytes[..header_end])
            .map_err(|_| NanowebError::Body("non-utf8 part header".into()))?;
        let body_bytes = &part_bytes[header_end + 4..];
        let mut name = String::new();
        let mut filename = None;
        let mut content_type = None;
        for line in headers_str.split("\r\n") {
            if let Some((k, v)) = line.split_once(':') {
                let k = k.trim().to_ascii_lowercase();
                let v = v.trim();
                if k == "content-disposition" {
                    for piece in v.split(';') {
                        let piece = piece.trim();
                        if let Some(rest) = piece.strip_prefix("name=") {
                            name = rest.trim_matches('"').to_string();
                        } else if let Some(rest) = piece.strip_prefix("filename=") {
                            filename = Some(rest.trim_matches('"').to_string());
                        }
                    }
                } else if k == "content-type" {
                    content_type = Some(v.to_string());
                }
            }
        }
        parts.push(MultipartPart {
            name,
            filename,
            content_type,
            body: body_bytes.to_vec(),
        });
        idx += next + 2; // skip \r\n
                         // now idx points at '--<boundary>'
        if s[idx..].starts_with(close_b) {
            return Ok(parts);
        }
        idx += bdy.len();
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Headers, Method, Request};

    fn req_with(ctype: &str, body: Vec<u8>) -> Request {
        let mut h = Headers::new();
        h.insert("Content-Type", ctype);
        Request {
            method: Method::Post,
            target: "/".into(),
            path: "/".into(),
            query: String::new(),
            version: "HTTP/1.1".into(),
            headers: h,
            body,
            params: Default::default(),
            remote_addr: None,
            request_id: String::new(),
            extensions: Default::default(),
        }
    }

    #[test]
    fn json_body() {
        #[derive(serde::Deserialize)]
        struct V {
            a: i32,
        }
        let r = req_with("application/json", b"{\"a\":7}".to_vec());
        let v: V = parse_body(&r).unwrap();
        assert_eq!(v.a, 7);
    }

    #[test]
    fn urlencoded_body() {
        let r = req_with("application/x-www-form-urlencoded", b"a=1&b=two".to_vec());
        #[derive(serde::Deserialize)]
        struct V {
            a: String,
            b: String,
        }
        let v: V = parse_body(&r).unwrap();
        assert_eq!(v.a, "1");
        assert_eq!(v.b, "two");
    }

    #[test]
    fn multipart_body() {
        let body = b"--BOUND\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nhello\r\n--BOUND\r\nContent-Disposition: form-data; name=\"b\"\r\n\r\nworld\r\n--BOUND--\r\n";
        let r = req_with("multipart/form-data; boundary=BOUND", body.to_vec());
        let parts = parse_multipart(&r.body, "BOUND").unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name, "a");
        assert_eq!(parts[0].body, b"hello");
        assert_eq!(parts[1].body, b"world");
    }
}

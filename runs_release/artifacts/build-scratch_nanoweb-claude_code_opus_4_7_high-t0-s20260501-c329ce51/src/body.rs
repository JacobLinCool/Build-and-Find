use crate::error::{NanowebError, Result};
use crate::http::Request;
use serde::de::DeserializeOwned;
use std::collections::HashMap;

/// Top-level body parsing entry point. Selects a parser based on the request
/// `Content-Type` and decodes the body into `T`.
pub fn parse_body<T: DeserializeOwned>(req: &Request) -> Result<T> {
    let ct = req
        .header("Content-Type")
        .unwrap_or("application/octet-stream")
        .to_string();
    let mime = ct
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "application/json" => parse_json(&req.body),
        "application/x-www-form-urlencoded" => parse_urlencoded(&req.body),
        s if s.starts_with("multipart/form-data") => {
            let boundary = boundary_from(&ct)
                .ok_or_else(|| NanowebError::Body("missing multipart boundary".into()))?;
            let form = parse_multipart(&req.body, &boundary)?;
            // Convert text fields to a JSON object then deserialize into T.
            let json: HashMap<String, serde_json::Value> = form
                .fields
                .into_iter()
                .map(|(k, v)| (k, serde_json::Value::String(v.value)))
                .collect();
            let v = serde_json::to_value(json)?;
            Ok(serde_json::from_value(v)?)
        }
        _ => Err(NanowebError::Body(format!(
            "unsupported content-type: {}",
            mime
        ))),
    }
}

pub fn parse_json<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    Ok(serde_json::from_slice(body)?)
}

pub fn parse_urlencoded<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    let pairs: HashMap<String, String> = form_urlencoded::parse(body).into_owned().collect();
    let v = serde_json::to_value(pairs).map_err(|e| NanowebError::Body(e.to_string()))?;
    Ok(serde_json::from_value(v)?)
}

#[derive(Debug, Default, Clone)]
pub struct MultipartForm {
    pub fields: Vec<(String, MultipartValue)>,
}

#[derive(Debug, Clone)]
pub struct MultipartValue {
    pub value: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub raw: Vec<u8>,
}

fn boundary_from(content_type: &str) -> Option<String> {
    for part in content_type.split(';') {
        let part = part.trim();
        if let Some(b) = part.strip_prefix("boundary=") {
            let b = b.trim_matches('"');
            return Some(b.to_string());
        }
    }
    None
}

pub fn parse_multipart(body: &[u8], boundary: &str) -> Result<MultipartForm> {
    let delim = format!("--{}", boundary);
    let close = format!("--{}--", boundary);
    let mut form = MultipartForm::default();

    // Find first delimiter
    let mut pos = match find(body, delim.as_bytes()) {
        Some(p) => p + delim.len(),
        None => return Err(NanowebError::Body("multipart: boundary not found".into())),
    };
    // skip optional CRLF
    if body[pos..].starts_with(b"\r\n") {
        pos += 2;
    }

    loop {
        // Check for closing delimiter at current position (preceded by --)
        if body[pos.saturating_sub(0)..].starts_with(b"--") {
            // already at "--" sequence after delim; reached end
            break;
        }
        // find next boundary
        let next_delim_pos = match find(&body[pos..], delim.as_bytes()) {
            Some(p) => pos + p,
            None => return Err(NanowebError::Body("multipart: missing delimiter".into())),
        };
        // part is body[pos..next_delim_pos] minus trailing CRLF before delim
        let mut part_end = next_delim_pos;
        if body[..part_end].ends_with(b"\r\n") {
            part_end -= 2;
        }
        let part = &body[pos..part_end];

        // split headers / body via CRLFCRLF
        let header_end = find(part, b"\r\n\r\n")
            .ok_or_else(|| NanowebError::Body("multipart: malformed part".into()))?;
        let header_str = std::str::from_utf8(&part[..header_end])
            .map_err(|_| NanowebError::Body("multipart: non-utf8 header".into()))?;
        let part_body = &part[header_end + 4..];

        let mut name = None::<String>;
        let mut filename = None::<String>;
        let mut content_type = None::<String>;
        for line in header_str.split("\r\n") {
            let lower = line.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("content-disposition:") {
                let _ = rest;
                let original = &line[line.find(':').unwrap() + 1..];
                for piece in original.split(';') {
                    let piece = piece.trim();
                    if let Some(v) = piece.strip_prefix("name=") {
                        name = Some(v.trim_matches('"').to_string());
                    } else if let Some(v) = piece.strip_prefix("filename=") {
                        filename = Some(v.trim_matches('"').to_string());
                    }
                }
            } else if let Some(rest) = lower.strip_prefix("content-type:") {
                let _ = rest;
                let original = &line[line.find(':').unwrap() + 1..];
                content_type = Some(original.trim().to_string());
            }
        }
        let name = name.ok_or_else(|| NanowebError::Body("multipart: missing name".into()))?;
        let value_str = if filename.is_some() {
            String::new()
        } else {
            String::from_utf8_lossy(part_body).into_owned()
        };
        form.fields.push((
            name,
            MultipartValue {
                value: value_str,
                filename,
                content_type,
                raw: part_body.to_vec(),
            },
        ));

        pos = next_delim_pos + delim.len();
        if body[pos..].starts_with(b"--") {
            break;
        }
        if body[pos..].starts_with(b"\r\n") {
            pos += 2;
        }
    }
    let _ = close;
    Ok(form)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    for i in 0..=haystack.len() - needle.len() {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, Version};
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Pair {
        a: String,
        b: String,
    }

    #[test]
    fn parses_json() {
        let mut r = Request::new(Method::Post, "/x".into(), Version::Http11);
        r.headers.insert("Content-Type", "application/json");
        r.body = br#"{"a":"1","b":"2"}"#.to_vec();
        let p: Pair = parse_body(&r).unwrap();
        assert_eq!(
            p,
            Pair {
                a: "1".into(),
                b: "2".into()
            }
        );
    }

    #[test]
    fn parses_urlencoded() {
        let mut r = Request::new(Method::Post, "/x".into(), Version::Http11);
        r.headers
            .insert("Content-Type", "application/x-www-form-urlencoded");
        r.body = b"a=1&b=2".to_vec();
        let p: Pair = parse_body(&r).unwrap();
        assert_eq!(
            p,
            Pair {
                a: "1".into(),
                b: "2".into()
            }
        );
    }

    #[test]
    fn parses_multipart() {
        let body = b"--BOUND\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n--BOUND\r\nContent-Disposition: form-data; name=\"b\"\r\n\r\n2\r\n--BOUND--\r\n";
        let f = parse_multipart(body, "BOUND").unwrap();
        assert_eq!(f.fields.len(), 2);
        assert_eq!(f.fields[0].0, "a");
        assert_eq!(f.fields[0].1.value, "1");
    }
}

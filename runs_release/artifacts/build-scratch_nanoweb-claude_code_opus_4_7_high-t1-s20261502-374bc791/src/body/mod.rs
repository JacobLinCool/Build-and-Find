//! Body parsers: JSON, urlencoded form, and multipart/form-data.

use crate::error::{NanowebError, Result};
use crate::http::Request;
use serde::de::DeserializeOwned;

pub mod multipart;

/// Decode application/x-www-form-urlencoded into a key/value list.
pub fn parse_query(input: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if input.is_empty() {
        return out;
    }
    for pair in input.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.find('=') {
            Some(i) => (&pair[..i], &pair[i + 1..]),
            None => (pair, ""),
        };
        out.push((urldecode(k), urldecode(v)));
    }
    out
}

/// Percent-decode a urlencoded string, treating '+' as space.
pub fn urldecode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_digit(bytes[i + 1]);
                let lo = hex_digit(bytes[i + 2]);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push(hi * 16 + lo);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// Determine the content-type of the body.
fn content_type(req: &Request) -> Option<String> {
    req.header("content-type").map(|s| {
        s.split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    })
}

/// Decode the request body to a serde-Deserialize-compatible value.
///
/// Selects the parser based on `Content-Type`:
///   - `application/json` -> serde_json
///   - `application/x-www-form-urlencoded` -> form -> json
///   - `multipart/form-data` -> field map -> json
pub fn parse_body<T: DeserializeOwned>(req: &Request) -> Result<T> {
    let ct = content_type(req).unwrap_or_default();
    if ct == "application/json" {
        return Ok(serde_json::from_slice(&req.body)?);
    }
    if ct == "application/x-www-form-urlencoded" {
        let s = std::str::from_utf8(&req.body)
            .map_err(|_| NanowebError::Body("non-utf8 form body".into()))?;
        let pairs = parse_query(s);
        let map: std::collections::BTreeMap<String, String> = pairs.into_iter().collect();
        return Ok(serde_json::from_value(serde_json::to_value(map)?)?);
    }
    if ct == "multipart/form-data" {
        let boundary = req
            .header("content-type")
            .and_then(multipart::extract_boundary)
            .ok_or_else(|| NanowebError::Body("missing multipart boundary".into()))?;
        let parts = multipart::parse(&req.body, &boundary)?;
        let mut map = std::collections::BTreeMap::new();
        for p in parts {
            // For typed access, we project text fields only. File fields are
            // exposed as a base64-encoded string under their field name.
            let value = if p.is_file {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.encode(&p.body)
            } else {
                String::from_utf8_lossy(&p.body).into_owned()
            };
            map.insert(p.name, value);
        }
        return Ok(serde_json::from_value(serde_json::to_value(map)?)?);
    }
    Err(NanowebError::Body(format!(
        "unsupported content-type: {}",
        ct
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_query() {
        let pairs = parse_query("a=1&b=hello%20world&c=%2B");
        assert_eq!(pairs[0], ("a".into(), "1".into()));
        assert_eq!(pairs[1], ("b".into(), "hello world".into()));
        assert_eq!(pairs[2], ("c".into(), "+".into()));
    }

    #[test]
    fn plus_is_space() {
        let pairs = parse_query("q=hello+world");
        assert_eq!(pairs[0].1, "hello world");
    }

    #[test]
    fn json_body_round_trip() {
        let mut req = Request::new();
        req.headers.insert("content-type", "application/json");
        req.body = br#"{"a":1,"b":"x"}"#.to_vec();
        #[derive(serde::Deserialize)]
        struct V {
            a: i32,
            b: String,
        }
        let v: V = parse_body(&req).unwrap();
        assert_eq!(v.a, 1);
        assert_eq!(v.b, "x");
    }

    #[test]
    fn form_body_decoded() {
        let mut req = Request::new();
        req.headers
            .insert("content-type", "application/x-www-form-urlencoded");
        req.body = b"a=1&b=hello+world".to_vec();
        let v: std::collections::BTreeMap<String, String> = parse_body(&req).unwrap();
        assert_eq!(v.get("b").unwrap(), "hello world");
    }
}

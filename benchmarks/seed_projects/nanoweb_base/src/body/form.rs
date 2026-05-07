//! `application/x-www-form-urlencoded` body parsing.

use std::collections::HashMap;

use crate::error::NanowebError;
use crate::http::types::Request;

/// Parse a url-encoded form body into a key/value map. The last value
/// for a repeated key wins.
pub fn parse_form(req: &Request) -> Result<HashMap<String, String>, NanowebError> {
    let body = std::str::from_utf8(&req.body)
        .map_err(|_| NanowebError::Body("form body is not utf-8".into()))?;
    decode_query_string(body)
}

/// Decode a url-encoded query string into a key-value map.
pub fn decode_query_string(raw: &str) -> Result<HashMap<String, String>, NanowebError> {
    let mut out = HashMap::new();
    if raw.is_empty() {
        return Ok(out);
    }
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.find('=') {
            Some(i) => (&pair[..i], &pair[i + 1..]),
            None => (pair, ""),
        };
        let key = percent_decode(k)?;
        let val = percent_decode(v)?;
        out.insert(key, val);
    }
    Ok(out)
}

fn percent_decode(raw: &str) -> Result<String, NanowebError> {
    let mut out = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(NanowebError::Body("truncated percent escape".into()));
                }
                let h1 = hex_nibble(bytes[i + 1])?;
                let h2 = hex_nibble(bytes[i + 2])?;
                out.push((h1 << 4) | h2);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| NanowebError::Body("percent decode yielded non-utf8".into()))
}

fn hex_nibble(c: u8) -> Result<u8, NanowebError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(NanowebError::Body("bad hex digit in percent escape".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::Method;

    #[test]
    fn decodes_spaces_and_ampersand() {
        let m = decode_query_string("name=John+Doe&msg=hi%20there").unwrap();
        assert_eq!(m.get("name").unwrap(), "John Doe");
        assert_eq!(m.get("msg").unwrap(), "hi there");
    }

    #[test]
    fn parse_form_handles_request_body() {
        let mut r = Request::new(Method::Post, "/");
        r.body = b"a=1&b=two".to_vec();
        let m = parse_form(&r).unwrap();
        assert_eq!(m.get("a").unwrap(), "1");
        assert_eq!(m.get("b").unwrap(), "two");
    }
}

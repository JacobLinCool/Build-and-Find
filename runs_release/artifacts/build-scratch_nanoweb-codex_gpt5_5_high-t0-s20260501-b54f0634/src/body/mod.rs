use crate::{error::Result, http::Request};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub fn parse_typed<T: DeserializeOwned>(request: &Request) -> Result<T> {
    let content_type = request.header("content-type").unwrap_or("");
    if content_type.starts_with("application/json") {
        Ok(serde_json::from_slice(&request.body)?)
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        let value: T = serde_urlencoded::from_bytes(&request.body)
            .map_err(|e| crate::error::NanowebError::BadRequest(e.to_string()))?;
        Ok(value)
    } else if content_type.starts_with("multipart/form-data") {
        let boundary = content_type
            .split(';')
            .find_map(|part| part.trim().strip_prefix("boundary="))
            .ok_or_else(|| crate::error::NanowebError::BadRequest("missing boundary".into()))?;
        let fields = parse_multipart(&request.body, boundary)?;
        let mut map = Map::new();
        for (key, val) in fields {
            map.insert(key, Value::String(val));
        }
        Ok(serde_json::from_value(Value::Object(map))?)
    } else {
        Err(crate::error::NanowebError::BadRequest(
            "unsupported content type".into(),
        ))
    }
}

pub fn parse_multipart(body: &[u8], boundary: &str) -> Result<HashMap<String, String>> {
    let text = String::from_utf8(body.to_vec())
        .map_err(|_| crate::error::NanowebError::BadRequest("multipart is not utf-8".into()))?;
    let marker = format!("--{boundary}");
    let mut fields = HashMap::new();
    for part in text.split(&marker).skip(1) {
        let part = part.trim_start_matches("\r\n");
        if part.starts_with("--") || part.trim().is_empty() {
            continue;
        }
        let Some((headers, value)) = part.split_once("\r\n\r\n") else {
            continue;
        };
        let name = headers.lines().find_map(|line| {
            let lower = line.to_ascii_lowercase();
            if !lower.starts_with("content-disposition:") {
                return None;
            }
            line.split(';').find_map(|attr| {
                attr.trim()
                    .strip_prefix("name=\"")
                    .and_then(|s| s.strip_suffix('"'))
                    .map(str::to_string)
            })
        });
        if let Some(name) = name {
            fields.insert(name, value.trim_end_matches("\r\n").to_string());
        }
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Form {
        name: String,
    }

    #[test]
    fn parses_urlencoded() {
        let mut req = Request::new(crate::http::Method::Post, "/");
        req.set_header("Content-Type", "application/x-www-form-urlencoded");
        req.body = b"name=nanoweb".to_vec();
        let form: Form = parse_typed(&req).unwrap();
        assert_eq!(form.name, "nanoweb");
    }

    #[test]
    fn parses_multipart_fields() {
        let body =
            b"--abc\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nhello\r\n--abc--\r\n";
        let fields = parse_multipart(body, "abc").unwrap();
        assert_eq!(fields.get("title"), Some(&"hello".to_string()));
    }
}

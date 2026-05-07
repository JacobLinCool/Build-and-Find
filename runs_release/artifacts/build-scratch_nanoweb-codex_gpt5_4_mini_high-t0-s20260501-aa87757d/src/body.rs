use crate::{error::Result, http::types::Request};
use base64::Engine;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

pub fn decode_body<T: DeserializeOwned>(request: &Request) -> Result<T> {
    let content_type = request
        .header("content-type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    match content_type.as_str() {
        "application/json" => Ok(serde_json::from_slice(&request.body)?),
        "application/x-www-form-urlencoded" => Ok(serde_urlencoded::from_bytes(&request.body)?),
        "multipart/form-data" => {
            let boundary =
                boundary_from_content_type(request.header("content-type").unwrap_or(""))?;
            let value = multipart_to_value(&request.body, &boundary)?;
            Ok(serde_json::from_value(value)?)
        }
        other => Err(crate::error::NanowebError::Body(format!(
            "unsupported content type {other}"
        ))),
    }
}

fn boundary_from_content_type(content_type: &str) -> Result<String> {
    for part in content_type.split(';') {
        let trimmed = part.trim();
        if let Some(boundary) = trimmed.strip_prefix("boundary=") {
            return Ok(boundary.trim_matches('"').to_string());
        }
    }
    Err(crate::error::NanowebError::Body(
        "multipart/form-data missing boundary".to_string(),
    ))
}

fn multipart_to_value(body: &[u8], boundary: &str) -> Result<Value> {
    let boundary_bytes = format!("--{}", boundary).into_bytes();
    let closing_bytes = format!("--{}--", boundary).into_bytes();
    let mut cursor = 0usize;
    let mut fields: Map<String, Value> = Map::new();

    while cursor < body.len() {
        if body[cursor..].starts_with(b"\r\n") {
            cursor += 2;
        }
        if body[cursor..].starts_with(&closing_bytes) {
            break;
        }
        if !body[cursor..].starts_with(&boundary_bytes) {
            return Err(crate::error::NanowebError::Body(
                "invalid multipart boundary".to_string(),
            ));
        }
        cursor += boundary_bytes.len();
        if body[cursor..].starts_with(b"--") {
            break;
        }
        if !body[cursor..].starts_with(b"\r\n") {
            return Err(crate::error::NanowebError::Body(
                "multipart boundary missing line break".to_string(),
            ));
        }
        cursor += 2;

        let headers_end = find_bytes(&body[cursor..], b"\r\n\r\n").ok_or_else(|| {
            crate::error::NanowebError::Body("multipart part missing header terminator".to_string())
        })?;
        let headers_raw =
            std::str::from_utf8(&body[cursor..cursor + headers_end]).map_err(|_| {
                crate::error::NanowebError::Body("multipart headers must be utf-8".to_string())
            })?;
        let mut name = None::<String>;
        let mut filename = None::<String>;
        let mut content_type = None::<String>;
        for line in headers_raw.split("\r\n") {
            let (header_name, header_value) = line.split_once(':').ok_or_else(|| {
                crate::error::NanowebError::Body("malformed multipart header".to_string())
            })?;
            match header_name.trim().to_ascii_lowercase().as_str() {
                "content-disposition" => {
                    for item in header_value.split(';') {
                        let item = item.trim();
                        if let Some(v) = item.strip_prefix("name=") {
                            name = Some(v.trim_matches('"').to_string());
                        } else if let Some(v) = item.strip_prefix("filename=") {
                            filename = Some(v.trim_matches('"').to_string());
                        }
                    }
                }
                "content-type" => {
                    content_type = Some(header_value.trim().to_string());
                }
                _ => {}
            }
        }
        let name = name.ok_or_else(|| {
            crate::error::NanowebError::Body("multipart part missing name".to_string())
        })?;
        cursor += headers_end + 4;
        let next_boundary = find_boundary(body, cursor, &boundary_bytes, &closing_bytes)
            .ok_or_else(|| {
                crate::error::NanowebError::Body("multipart part missing end boundary".to_string())
            })?;
        let mut part = body[cursor..next_boundary].to_vec();
        if part.ends_with(b"\r\n") {
            part.truncate(part.len().saturating_sub(2));
        }
        let value = if let Some(filename) = filename {
            let mut obj = Map::new();
            obj.insert("filename".to_string(), Value::String(filename));
            if let Some(content_type) = content_type {
                obj.insert("content_type".to_string(), Value::String(content_type));
            }
            obj.insert(
                "data".to_string(),
                Value::String(base64::engine::general_purpose::STANDARD.encode(part)),
            );
            Value::Object(obj)
        } else {
            coerce_scalar(&String::from_utf8_lossy(&part))
        };
        insert_multipart_field(&mut fields, name, value);
        cursor = next_boundary;
    }

    Ok(Value::Object(fields))
}

fn insert_multipart_field(fields: &mut Map<String, Value>, name: String, value: Value) {
    if let Some(existing) = fields.get_mut(&name) {
        match existing {
            Value::Array(items) => items.push(value),
            other => {
                let previous = other.take();
                *other = Value::Array(vec![previous, value]);
            }
        }
    } else {
        fields.insert(name, value);
    }
}

fn coerce_scalar(value: &str) -> Value {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if let Ok(number) = trimmed.parse::<i64>() {
        return Value::Number(number.into());
    }
    if let Ok(number) = trimmed.parse::<u64>() {
        return Value::Number(number.into());
    }
    if let Ok(number) = trimmed.parse::<f64>() {
        if let Some(number) = serde_json::Number::from_f64(number) {
            return Value::Number(number);
        }
    }
    Value::String(trimmed.to_string())
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn find_boundary(
    body: &[u8],
    start: usize,
    boundary_bytes: &[u8],
    closing_bytes: &[u8],
) -> Option<usize> {
    let mut i = start;
    while i + 2 <= body.len() {
        if body[i..].starts_with(b"\r\n") {
            if body[i + 2..].starts_with(boundary_bytes) || body[i + 2..].starts_with(closing_bytes)
            {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::decode_body;
    use crate::http::types::{Method, Request, Version};
    use std::collections::HashMap;

    #[test]
    fn decode_json() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        let req = Request {
            method: Method::POST,
            target: "/".to_string(),
            path: "/".to_string(),
            query: None,
            version: Version::Http11,
            headers,
            body: br#"{"name":"nanoweb"}"#.to_vec(),
            params: HashMap::new(),
            request_id: "test".to_string(),
            remote_addr: None,
            route_pattern: None,
            connection_close: false,
        };
        let value: serde_json::Value = decode_body(&req).unwrap();
        assert_eq!(value["name"], "nanoweb");
    }
}

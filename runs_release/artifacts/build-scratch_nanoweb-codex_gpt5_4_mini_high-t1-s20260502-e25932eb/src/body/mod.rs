use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::error::{NanowebError, Result};

pub fn parse_body<T: DeserializeOwned>(content_type: Option<&str>, body: &[u8]) -> Result<T> {
    let content_type = content_type
        .ok_or_else(|| NanowebError::Body("content-type is required for body parsing".into()))?
        .to_ascii_lowercase();

    if content_type.starts_with("application/json") {
        Ok(serde_json::from_slice(body)?)
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        Ok(serde_urlencoded::from_bytes(body)?)
    } else if content_type.starts_with("multipart/form-data") {
        let boundary = extract_boundary(&content_type)
            .ok_or_else(|| NanowebError::Body("multipart boundary missing".into()))?;
        let value = parse_multipart_value(body, &boundary)?;
        Ok(serde_json::from_value(value)?)
    } else {
        Err(NanowebError::Body(format!(
            "unsupported content-type: {content_type}"
        )))
    }
}

fn extract_boundary(content_type: &str) -> Option<String> {
    content_type.split(';').find_map(|part| {
        let part = part.trim();
        part.strip_prefix("boundary=")
            .map(|boundary| boundary.trim_matches('"').to_string())
    })
}

fn parse_multipart_value(body: &[u8], boundary: &str) -> Result<Value> {
    let body = String::from_utf8_lossy(body);
    let delimiter = format!("--{boundary}");
    let mut map = Map::new();
    let mut sections = body.split(&delimiter);
    let _ = sections.next();
    for section in sections {
        let section = section.trim_start_matches("\r\n");
        if section.starts_with("--") {
            break;
        }
        let section = section.trim_end_matches("\r\n");
        if section.is_empty() {
            continue;
        }
        let (headers, content) = section
            .split_once("\r\n\r\n")
            .ok_or_else(|| NanowebError::Body("invalid multipart section".into()))?;
        let field_name = headers
            .lines()
            .find_map(parse_field_name)
            .ok_or_else(|| NanowebError::Body("multipart field name missing".into()))?;
        let value = Value::String(content.trim_end_matches("\r\n").to_string());
        insert_field(&mut map, field_name, value);
    }
    Ok(Value::Object(map))
}

fn parse_field_name(header_line: &str) -> Option<String> {
    let line = header_line.to_ascii_lowercase();
    if !line.starts_with("content-disposition:") {
        return None;
    }
    header_line
        .split(';')
        .find_map(|segment| segment.trim().strip_prefix("name="))
        .map(|name| name.trim_matches('"').to_string())
}

fn insert_field(map: &mut Map<String, Value>, key: String, value: Value) {
    match map.get_mut(&key) {
        Some(existing) => match existing {
            Value::Array(values) => values.push(value),
            other => {
                let previous = std::mem::take(other);
                *other = Value::Array(vec![previous, value]);
            }
        },
        None => {
            map.insert(key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct LoginForm {
        username: String,
        password: String,
    }

    #[test]
    fn parses_json() {
        let parsed: LoginForm = parse_body(
            Some("application/json"),
            br#"{"username":"a","password":"b"}"#,
        )
        .unwrap();
        assert_eq!(parsed.username, "a");
    }

    #[test]
    fn parses_urlencoded() {
        let parsed: LoginForm = parse_body(
            Some("application/x-www-form-urlencoded"),
            b"username=a&password=b",
        )
        .unwrap();
        assert_eq!(parsed.password, "b");
    }

    #[test]
    fn parses_multipart() {
        let body = concat!(
            "--boundary\r\n",
            "Content-Disposition: form-data; name=\"username\"\r\n\r\n",
            "alice\r\n",
            "--boundary\r\n",
            "Content-Disposition: form-data; name=\"password\"\r\n\r\n",
            "secret\r\n",
            "--boundary--\r\n"
        );
        let parsed: LoginForm = parse_body(
            Some("multipart/form-data; boundary=boundary"),
            body.as_bytes(),
        )
        .unwrap();
        assert_eq!(parsed.username, "alice");
    }
}

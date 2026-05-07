use std::collections::HashMap;

use crate::error::{NanowebError, Result};
use crate::http::Request;

pub fn parse_body<T: serde::de::DeserializeOwned>(req: &Request) -> Result<T> {
    let content_type = req.headers.content_type().unwrap_or("");
    let body_str = std::str::from_utf8(&req.body)
        .map_err(|e| NanowebError::BodyParse(format!("Invalid UTF-8: {}", e)))?;

    if content_type.contains("application/json") {
        serde_json::from_str(body_str)
            .map_err(|e| NanowebError::BodyParse(format!("JSON parse error: {}", e)))
    } else if content_type.contains("application/x-www-form-urlencoded") {
        let pairs: HashMap<String, String> = body_str
            .split('&')
            .filter_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                let key = parts.next()?.to_string();
                let val = parts.next().unwrap_or("").to_string();
                Some((key, val))
            })
            .collect();
        serde_json::from_value(serde_json::to_value(&pairs).unwrap())
            .map_err(|e| NanowebError::BodyParse(format!("Form parse error: {}", e)))
    } else if content_type.contains("multipart/form-data") {
        let boundary = extract_boundary(content_type)
            .ok_or_else(|| NanowebError::BodyParse("Missing multipart boundary".into()))?;
        parse_multipart(body_str.as_bytes(), &boundary).and_then(|fields| {
            serde_json::from_value(serde_json::to_value(&fields).unwrap())
                .map_err(|e| NanowebError::BodyParse(format!("Multipart parse error: {}", e)))
        })
    } else {
        serde_json::from_str(body_str)
            .map_err(|e| NanowebError::BodyParse(format!("Body parse error: {}", e)))
    }
}

fn extract_boundary(content_type: &str) -> Option<String> {
    content_type.split(';').find_map(|part| {
        let part = part.trim();
        if part.starts_with("boundary=") {
            Some(part[9..].trim_matches('"').to_string())
        } else {
            None
        }
    })
}

fn parse_multipart(body: &[u8], boundary: &str) -> Result<Vec<(String, String)>> {
    let mut fields = Vec::new();
    let sep = format!("--{}", boundary);
    let _end = format!("--{}--", boundary);

    let body_str = std::str::from_utf8(body)
        .map_err(|e| NanowebError::BodyParse(format!("Invalid UTF-8 in multipart: {}", e)))?;
    let parts: Vec<&str> = body_str.split(&sep).collect();

    for part in parts {
        let part = part.trim_start_matches('\r').trim_start_matches('\n');
        if part.is_empty() || part == "--" || part.starts_with("--") {
            continue;
        }
        let part = part.trim_end_matches('\r').trim_end_matches('\n');

        if let Some(headers_end) = part.find("\r\n\r\n") {
            let headers_str = &part[..headers_end];
            let value = part[headers_end + 4..].trim();

            let name = headers_str
                .lines()
                .find_map(|line| {
                    if line.to_lowercase().starts_with("content-disposition:") {
                        line.split(';').find_map(|s| {
                            let s = s.trim();
                            if s.starts_with("name=\"") {
                                let val = s[6..].trim_matches('"');
                                Some(val.to_string())
                            } else {
                                None
                            }
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or_default();

            fields.push((name, value.to_string()));
        }
    }

    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Headers, Method};

    #[test]
    fn parse_json_body() {
        let mut req = Request::new(Method::Post, "/api".into());
        req.headers
            .insert("content-type".into(), "application/json".into());
        req.body = br#"{"name":"test","age":30}"#.to_vec();

        #[derive(serde::Deserialize, Debug)]
        struct Data {
            name: String,
            age: u32,
        }

        let data: Data = req.body_as().unwrap();
        assert_eq!(data.name, "test");
        assert_eq!(data.age, 30);
    }

    #[test]
    fn parse_form_body() {
        let mut req = Request::new(Method::Post, "/api".into());
        req.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        req.body = b"name=test&age=30".to_vec();

        #[derive(serde::Deserialize, Debug)]
        struct Data {
            name: String,
            age: String,
        }

        let data: Data = req.body_as().unwrap();
        assert_eq!(data.name, "test");
        assert_eq!(data.age, "30");
    }
}

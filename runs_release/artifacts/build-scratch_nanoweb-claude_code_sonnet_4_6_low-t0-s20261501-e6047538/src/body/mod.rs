use crate::error::NanowebError;
use crate::http::types::Request;
use std::collections::HashMap;

/// Parse JSON body
pub fn parse_json(req: &Request) -> Result<serde_json::Value, NanowebError> {
    let body_str = req
        .body_str()
        .map_err(|_| NanowebError::BadRequest("invalid UTF-8 body".to_string()))?;
    serde_json::from_str(body_str).map_err(|e| NanowebError::ParseError(e.to_string()))
}

/// Parse URL-encoded form body
pub fn parse_urlencoded(req: &Request) -> Result<HashMap<String, String>, NanowebError> {
    let body_str = req
        .body_str()
        .map_err(|_| NanowebError::BadRequest("invalid UTF-8 body".to_string()))?;
    let mut map = HashMap::new();

    for pair in body_str.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = if let Some(eq) = pair.find('=') {
            (&pair[..eq], &pair[eq + 1..])
        } else {
            (pair, "")
        };
        let key = percent_decode(key)?;
        let value = percent_decode(value)?;
        map.insert(key, value);
    }

    Ok(map)
}

fn percent_decode(s: &str) -> Result<String, NanowebError> {
    percent_encoding::percent_decode_str(&s.replace('+', " "))
        .decode_utf8()
        .map(|c| c.into_owned())
        .map_err(|e| NanowebError::ParseError(e.to_string()))
}

/// A parsed multipart form field
#[derive(Debug, Clone)]
pub struct MultipartField {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

/// Parse multipart/form-data body
pub fn parse_multipart(req: &Request) -> Result<Vec<MultipartField>, NanowebError> {
    let ct = req
        .content_type()
        .ok_or_else(|| NanowebError::BadRequest("missing content-type".to_string()))?;

    let boundary = extract_boundary(ct)
        .ok_or_else(|| NanowebError::BadRequest("missing multipart boundary".to_string()))?;

    parse_multipart_body(&req.body, &boundary)
}

fn extract_boundary(ct: &str) -> Option<String> {
    for part in ct.split(';') {
        let part = part.trim();
        if let Some(b) = part.strip_prefix("boundary=") {
            return Some(b.trim_matches('"').to_string());
        }
    }
    None
}

fn parse_multipart_body(body: &[u8], boundary: &str) -> Result<Vec<MultipartField>, NanowebError> {
    let delimiter = format!("--{}", boundary);
    let body_str = std::str::from_utf8(body)
        .map_err(|_| NanowebError::BadRequest("invalid UTF-8 in multipart body".to_string()))?;

    let mut fields = Vec::new();

    let parts: Vec<&str> = body_str.split(&delimiter).collect();
    for part in &parts[1..] {
        let part = part.trim_start_matches("\r\n");
        if part.starts_with("--") || part == "--\r\n" || part.trim() == "--" {
            break;
        }

        // Find header/body separator
        let sep = part
            .find("\r\n\r\n")
            .ok_or_else(|| NanowebError::BadRequest("invalid multipart part".to_string()))?;

        let headers_str = &part[..sep];
        let body_part = &part[sep + 4..];
        // Remove trailing \r\n
        let body_part = body_part.trim_end_matches("\r\n");

        let mut name = String::new();
        let mut filename = None;
        let mut content_type = None;

        for header_line in headers_str.lines() {
            let header_lower = header_line.to_lowercase();
            if header_lower.starts_with("content-disposition:") {
                let rest = &header_line["content-disposition:".len()..];
                for item in rest.split(';') {
                    let item = item.trim();
                    if let Some(n) = item.strip_prefix("name=") {
                        name = n.trim_matches('"').to_string();
                    } else if let Some(f) = item.strip_prefix("filename=") {
                        filename = Some(f.trim_matches('"').to_string());
                    }
                }
            } else if header_lower.starts_with("content-type:") {
                content_type = Some(header_line["content-type:".len()..].trim().to_string());
            }
        }

        if !name.is_empty() {
            fields.push(MultipartField {
                name,
                filename,
                content_type,
                data: body_part.as_bytes().to_vec(),
            });
        }
    }

    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{Method, Request};

    #[test]
    fn test_parse_json() {
        let mut req = Request::new(Method::POST, "/".to_string());
        req.body = b"{\"key\": \"value\"}".to_vec();
        req.headers
            .insert("content-type".to_string(), "application/json".to_string());

        let val = parse_json(&req).unwrap();
        assert_eq!(val["key"], "value");
    }

    #[test]
    fn test_parse_urlencoded() {
        let mut req = Request::new(Method::POST, "/".to_string());
        req.body = b"name=john&age=30".to_vec();

        let map = parse_urlencoded(&req).unwrap();
        assert_eq!(map.get("name"), Some(&"john".to_string()));
        assert_eq!(map.get("age"), Some(&"30".to_string()));
    }

    #[test]
    fn test_parse_urlencoded_encoded() {
        let mut req = Request::new(Method::POST, "/".to_string());
        req.body = b"name=hello+world&city=New%20York".to_vec();

        let map = parse_urlencoded(&req).unwrap();
        assert_eq!(map.get("name"), Some(&"hello world".to_string()));
        assert_eq!(map.get("city"), Some(&"New York".to_string()));
    }
}

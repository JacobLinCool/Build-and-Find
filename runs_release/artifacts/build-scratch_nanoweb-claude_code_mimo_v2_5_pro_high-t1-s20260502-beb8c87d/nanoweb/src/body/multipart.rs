use crate::error::NanowebError;
use crate::http::types::Request;
use serde::de::DeserializeOwned;

#[derive(Debug, Clone)]
pub struct MultipartField {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

pub fn parse_multipart(req: &Request) -> Result<Vec<MultipartField>, NanowebError> {
    let boundary = extract_boundary(req.content_type().unwrap_or(""))?;
    let body = &req.body;

    let boundary_delim = format!("--{boundary}");
    let end_delim = format!("--{boundary}--");

    let body_str = String::from_utf8_lossy(body);
    let parts: Vec<&str> = body_str.split(&boundary_delim).collect();

    let mut fields = Vec::new();

    for part in &parts[1..] {
        let part = part.trim_start_matches('\r').trim_start_matches('\n');
        if part.starts_with("--") || part == end_delim.as_str() {
            continue;
        }

        if let Some((header_section, body_section)) = split_once(part, "\r\n\r\n") {
            let field = parse_part(&header_section, body_section.as_bytes());
            fields.push(field);
        }
    }

    Ok(fields)
}

fn split_once(s: &str, delim: &str) -> Option<(String, String)> {
    let pos = s.find(delim)?;
    Some((s[..pos].to_string(), s[pos + delim.len()..].to_string()))
}

fn parse_part(headers_str: &str, body: &[u8]) -> MultipartField {
    let mut name = String::new();
    let mut filename = None;
    let mut content_type = None;

    for line in headers_str.split("\r\n") {
        let lower = line.to_lowercase();
        if lower.starts_with("content-disposition:") {
            if let Some(n) = extract_param(line, "name") {
                name = n;
            }
            filename = extract_param(line, "filename");
        } else if lower.starts_with("content-type:") {
            content_type = Some(line.splitn(2, ':').nth(1).unwrap_or("").trim().to_string());
        }
    }

    // Trim trailing \r\n from body
    let body = if body.ends_with(b"\r\n") {
        &body[..body.len() - 2]
    } else {
        body
    };

    MultipartField {
        name,
        filename,
        content_type,
        data: body.to_vec(),
    }
}

fn extract_param(header: &str, param: &str) -> Option<String> {
    let needle = format!("{param}=\"");
    if let Some(pos) = header.find(&needle) {
        let start = pos + needle.len();
        if let Some(end) = header[start..].find('"') {
            return Some(header[start..start + end].to_string());
        }
    }
    None
}

fn extract_boundary(content_type: &str) -> Result<String, NanowebError> {
    for part in content_type.split(';') {
        let part = part.trim();
        if part.starts_with("boundary=") {
            let boundary = part[9..].trim().trim_matches('"');
            return Ok(boundary.to_string());
        }
    }
    Err(NanowebError::Body(
        "Missing boundary in multipart content type".to_string(),
    ))
}

pub fn parse_multipart_json<T: DeserializeOwned>(req: &Request) -> Result<T, NanowebError> {
    // For multipart, collect fields into a JSON object
    let fields = parse_multipart(req)?;
    let mut map = serde_json::Map::new();
    for field in &fields {
        if let Ok(s) = std::str::from_utf8(&field.data) {
            map.insert(field.name.clone(), serde_json::Value::String(s.to_string()));
        }
    }
    let value = serde_json::Value::Object(map);
    serde_json::from_value(value)
        .map_err(|e| NanowebError::Body(format!("Multipart parse error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_extract_boundary() {
        let ct = "multipart/form-data; boundary=----WebKitFormBoundary";
        assert_eq!(extract_boundary(ct).unwrap(), "----WebKitFormBoundary");
    }

    #[test]
    fn test_parse_multipart() {
        let body = b"------WebKitFormBoundary\r\n\
            Content-Disposition: form-data; name=\"field1\"\r\n\r\n\
            value1\r\n\
            ------WebKitFormBoundary\r\n\
            Content-Disposition: form-data; name=\"field2\"; filename=\"test.txt\"\r\n\
            Content-Type: text/plain\r\n\r\n\
            file content\r\n\
            ------WebKitFormBoundary--\r\n";

        let boundary = "----WebKitFormBoundary";
        let ct = format!("multipart/form-data; boundary={boundary}");
        let mut headers = std::collections::HashMap::new();
        headers.insert("content-type".to_string(), ct);

        let req = Request {
            method: crate::http::types::Method::Post,
            target: "/".to_string(),
            path: "/".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers,
            params: HashMap::new(),
            body: body.to_vec(),
            request_id: "test".to_string(),
            remote_addr: None,
        };

        let fields = parse_multipart(&req).unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "field1");
        assert_eq!(String::from_utf8_lossy(&fields[0].data), "value1");
        assert_eq!(fields[1].name, "field2");
        assert_eq!(fields[1].filename.as_deref(), Some("test.txt"));
    }
}

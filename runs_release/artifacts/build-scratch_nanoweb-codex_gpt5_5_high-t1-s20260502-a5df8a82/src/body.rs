use std::collections::BTreeMap;

use serde::de::DeserializeOwned;

use crate::error::{NanowebError, Result};
use crate::http::Request;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartPart {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

impl Request {
    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T> {
        let content_type = self
            .header("content-type")
            .unwrap_or("application/octet-stream")
            .to_ascii_lowercase();
        if content_type.starts_with("application/json") {
            Ok(serde_json::from_slice(&self.body)?)
        } else if content_type.starts_with("application/x-www-form-urlencoded")
            || content_type.starts_with("application/x- www-form-urlencoded")
        {
            serde_urlencoded::from_bytes(&self.body)
                .map_err(|e| NanowebError::Parse(format!("form body error: {e}")))
        } else {
            Err(NanowebError::Parse(format!(
                "unsupported content type: {content_type}"
            )))
        }
    }

    pub fn form_fields(&self) -> Result<BTreeMap<String, String>> {
        serde_urlencoded::from_bytes(&self.body)
            .map_err(|e| NanowebError::Parse(format!("form body error: {e}")))
    }

    pub fn multipart(&self) -> Result<Vec<MultipartPart>> {
        let content_type = self
            .header("content-type")
            .ok_or_else(|| NanowebError::Parse("missing multipart content type".into()))?;
        let boundary = content_type
            .split(';')
            .map(str::trim)
            .find_map(|part| part.strip_prefix("boundary="))
            .ok_or_else(|| NanowebError::Parse("missing multipart boundary".into()))?;
        parse_multipart(&self.body, boundary)
    }
}

pub fn parse_multipart(body: &[u8], boundary: &str) -> Result<Vec<MultipartPart>> {
    let marker = format!("--{boundary}");
    let text = std::str::from_utf8(body)
        .map_err(|_| NanowebError::Parse("multipart body is not UTF-8".into()))?;
    let mut parts = Vec::new();
    for raw in text.split(&marker).skip(1) {
        let raw = raw.trim_start_matches("\r\n");
        if raw.starts_with("--") || raw.trim().is_empty() {
            continue;
        }
        let raw = raw.trim_end_matches("\r\n");
        let (header_text, data) = raw
            .split_once("\r\n\r\n")
            .ok_or_else(|| NanowebError::Parse("malformed multipart part".into()))?;
        let mut name = None;
        let mut filename = None;
        let mut content_type = None;
        for line in header_text.split("\r\n") {
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| NanowebError::Parse("malformed multipart header".into()))?;
            if key.eq_ignore_ascii_case("content-disposition") {
                for attr in value.split(';').map(str::trim) {
                    if let Some(v) = attr.strip_prefix("name=") {
                        name = Some(v.trim_matches('"').to_string());
                    } else if let Some(v) = attr.strip_prefix("filename=") {
                        filename = Some(v.trim_matches('"').to_string());
                    }
                }
            } else if key.eq_ignore_ascii_case("content-type") {
                content_type = Some(value.trim().to_string());
            }
        }
        parts.push(MultipartPart {
            name: name.ok_or_else(|| NanowebError::Parse("multipart part missing name".into()))?,
            filename,
            content_type,
            data: data.as_bytes().to_vec(),
        });
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::http::Method;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Name {
        name: String,
    }

    #[test]
    fn parses_json_body() {
        let mut req = Request::new(Method::Post, "/");
        req.set_header("Content-Type", "application/json");
        req.body = br#"{"name":"Ada"}"#.to_vec();
        assert_eq!(req.body_as::<Name>().unwrap().name, "Ada");
    }

    #[test]
    fn parses_form_body() {
        let mut req = Request::new(Method::Post, "/");
        req.set_header("Content-Type", "application/x-www-form-urlencoded");
        req.body = b"name=Ada".to_vec();
        assert_eq!(req.body_as::<Name>().unwrap().name, "Ada");
    }

    #[test]
    fn parses_multipart() {
        let body = b"--x\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\none\r\n--x--\r\n";
        let parts = parse_multipart(body, "x").unwrap();
        assert_eq!(parts[0].name, "a");
        assert_eq!(parts[0].data, b"one");
    }
}

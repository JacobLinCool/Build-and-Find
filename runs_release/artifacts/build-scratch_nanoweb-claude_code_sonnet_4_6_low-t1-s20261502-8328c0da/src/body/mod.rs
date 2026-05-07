use crate::error::NanowebError;
use crate::http::types::Request;
use serde::de::DeserializeOwned;

pub trait BodyParser {
    fn body_as<T: DeserializeOwned>(&self) -> Result<T, NanowebError>;
    fn body_as_json<T: DeserializeOwned>(&self) -> Result<T, NanowebError>;
    fn body_as_form<T: DeserializeOwned>(&self) -> Result<T, NanowebError>;
}

impl BodyParser for Request {
    fn body_as<T: DeserializeOwned>(&self) -> Result<T, NanowebError> {
        let content_type = self
            .headers
            .get("content-type")
            .unwrap_or("")
            .to_lowercase();

        if content_type.contains("application/json") {
            self.body_as_json()
        } else if content_type.contains("application/x-www-form-urlencoded") {
            self.body_as_form()
        } else if content_type.contains("multipart/form-data") {
            // For multipart, we just try JSON as a fallback
            Err(NanowebError::Parse(
                "Use body_as_multipart for multipart/form-data".to_string(),
            ))
        } else {
            // Try JSON by default
            self.body_as_json()
        }
    }

    fn body_as_json<T: DeserializeOwned>(&self) -> Result<T, NanowebError> {
        serde_json::from_slice(&self.body).map_err(NanowebError::SerdeJson)
    }

    fn body_as_form<T: DeserializeOwned>(&self) -> Result<T, NanowebError> {
        let body_str = std::str::from_utf8(&self.body)
            .map_err(|e| NanowebError::Parse(e.to_string()))?;
        serde_urlencoded::from_str(body_str)
            .map_err(|e| NanowebError::Parse(e.to_string()))
    }
}

/// Parse multipart form data
pub async fn parse_multipart(
    req: &Request,
) -> Result<Vec<MultipartField>, NanowebError> {
    let content_type = req
        .headers
        .get("content-type")
        .ok_or_else(|| NanowebError::Parse("Missing content-type".to_string()))?;

    let boundary = extract_boundary(content_type)
        .ok_or_else(|| NanowebError::Parse("Missing multipart boundary".to_string()))?;

    let body = bytes::Bytes::copy_from_slice(&req.body);
    let stream = futures_util_stream_once(body);

    let mut multipart = multer::Multipart::new(stream, boundary);
    let mut fields = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| NanowebError::Parse(e.to_string()))?
    {
        let name = field.name().map(|s| s.to_string()).unwrap_or_default();
        let filename = field.file_name().map(|s| s.to_string());
        let content_type = field.content_type().map(|ct| ct.to_string());
        let data = field
            .bytes()
            .await
            .map_err(|e| NanowebError::Parse(e.to_string()))?
            .to_vec();

        fields.push(MultipartField {
            name,
            filename,
            content_type,
            data,
        });
    }

    Ok(fields)
}

fn futures_util_stream_once(
    data: bytes::Bytes,
) -> impl tokio_stream::Stream<Item = Result<bytes::Bytes, std::io::Error>> {
    tokio_stream::once(Ok::<bytes::Bytes, std::io::Error>(data))
}

fn extract_boundary(content_type: &str) -> Option<String> {
    for part in content_type.split(';') {
        let part = part.trim();
        if let Some(b) = part.strip_prefix("boundary=") {
            return Some(b.trim_matches('"').to_string());
        }
    }
    None
}

#[derive(Debug)]
pub struct MultipartField {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{Headers, Method, Request};
    use std::collections::HashMap;

    fn make_request(body: &[u8], content_type: &str) -> Request {
        let mut headers = Headers::new();
        headers.set("content-type", content_type);
        Request::new(
            Method::Post,
            "/test".to_string(),
            HashMap::new(),
            headers,
            body.to_vec(),
            "127.0.0.1:0".parse().unwrap(),
        )
    }

    #[test]
    fn test_body_as_json() {
        let req = make_request(b"{\"name\":\"Alice\"}", "application/json");
        let val: serde_json::Value = req.body_as_json().unwrap();
        assert_eq!(val["name"], "Alice");
    }

    #[test]
    fn test_body_as_form() {
        let req = make_request(b"name=Alice&age=30", "application/x-www-form-urlencoded");
        let val: HashMap<String, String> = req.body_as_form().unwrap();
        assert_eq!(val.get("name"), Some(&"Alice".to_string()));
    }
}

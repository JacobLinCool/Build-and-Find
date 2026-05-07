pub mod json;
pub mod multipart;
pub mod urlencoded;

use crate::error::NanowebError;
use crate::http::types::Request;
use serde::de::DeserializeOwned;

pub trait BodyParser {
    fn parse<T: DeserializeOwned>(&self, req: &Request) -> Result<T, NanowebError>;
}

pub fn parse_body<T: DeserializeOwned>(req: &Request) -> Result<T, NanowebError> {
    let content_type = req.content_type().unwrap_or("application/octet-stream");

    if content_type.starts_with("application/json") {
        json::parse_json(req)
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        urlencoded::parse_urlencoded(req)
    } else if content_type.starts_with("multipart/form-data") {
        multipart::parse_multipart_json(req)
    } else {
        Err(NanowebError::Body(format!(
            "Unsupported content type: {content_type}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::collections::HashMap;

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestData {
        name: String,
        age: u32,
    }

    #[test]
    fn test_parse_json_body() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        let req = Request {
            method: crate::http::types::Method::Post,
            target: "/test".to_string(),
            path: "/test".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers,
            params: HashMap::new(),
            body: br#"{"name":"Alice","age":30}"#.to_vec(),
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let data: TestData = parse_body(&req).unwrap();
        assert_eq!(data.name, "Alice");
        assert_eq!(data.age, 30);
    }

    #[test]
    fn test_parse_urlencoded_body() {
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        let req = Request {
            method: crate::http::types::Method::Post,
            target: "/test".to_string(),
            path: "/test".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers,
            params: HashMap::new(),
            body: b"name=Alice&age=30".to_vec(),
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let data: TestData = parse_body(&req).unwrap();
        assert_eq!(data.name, "Alice");
        assert_eq!(data.age, 30);
    }
}

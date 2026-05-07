pub mod form;
pub mod json;
pub mod multipart;

use crate::error::{NanowebError, Result};
use crate::http::types::Request;
use serde::de::DeserializeOwned;

pub fn parse_body<T: DeserializeOwned>(req: &Request) -> Result<T> {
    let content_type = req.headers.get("content-type").unwrap_or("").to_string();

    if content_type.contains("application/json") {
        json::parse_json(&req.body)
    } else if content_type.contains("application/x-www-form-urlencoded") {
        form::parse_form(&req.body)
    } else if content_type.contains("multipart/form-data") {
        let boundary = extract_boundary(&content_type)
            .ok_or_else(|| NanowebError::Parse("Missing multipart boundary".into()))?;
        multipart::parse_multipart(&req.body, &boundary)
    } else {
        // Try JSON as default
        json::parse_json(&req.body)
    }
}

fn extract_boundary(content_type: &str) -> Option<String> {
    for part in content_type.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("boundary=") {
            return Some(val.trim_matches('"').to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestForm {
        name: String,
        value: String,
    }

    #[test]
    fn test_json_parse() {
        let mut req = Request::default();
        req.headers.insert("content-type", "application/json");
        req.body = br#"{"name":"test","value":"123"}"#.to_vec();
        let result: TestForm = parse_body(&req).unwrap();
        assert_eq!(result.name, "test");
        assert_eq!(result.value, "123");
    }
}

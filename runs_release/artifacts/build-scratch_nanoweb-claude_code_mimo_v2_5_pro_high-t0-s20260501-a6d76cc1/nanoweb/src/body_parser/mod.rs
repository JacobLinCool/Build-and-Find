use crate::error::NanowebError;
use crate::http::Request;
use serde::de::DeserializeOwned;

pub mod json;
pub mod multipart;
pub mod urlencoded;

impl Request {
    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T, NanowebError> {
        let content_type = self.content_type().unwrap_or("");
        if content_type.contains("application/json") {
            json::parse_json(&self.body)
        } else if content_type.contains("application/x-www-form-urlencoded") {
            urlencoded::parse_urlencoded(&self.body)
        } else {
            Err(NanowebError::Body(format!(
                "unsupported content type: {content_type}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn body_as_json() {
        let mut req = Request {
            method: crate::http::Method::Post,
            path: "/test".into(),
            query: None,
            version: "HTTP/1.1".into(),
            headers: HashMap::new(),
            body: b"{\"name\":\"test\"}".to_vec(),
            params: HashMap::new(),
            addr: "127.0.0.1:12345".parse().unwrap(),
            request_id: "test".into(),
        };
        req.headers
            .insert("content-type".into(), "application/json".into());
        let result: serde_json::Value = req.body_as().unwrap();
        assert_eq!(result["name"], "test");
    }
}

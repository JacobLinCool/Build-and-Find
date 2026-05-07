use crate::error::NanowebError;
use crate::http::types::Request;
use serde::de::DeserializeOwned;

pub fn parse_json<T: DeserializeOwned>(req: &Request) -> Result<T, NanowebError> {
    serde_json::from_slice(&req.body)
        .map_err(|e| NanowebError::Body(format!("JSON parse error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::collections::HashMap;

    #[derive(Deserialize, Debug)]
    struct Item {
        id: u32,
        name: String,
    }

    #[test]
    fn test_json_parse() {
        let req = Request {
            method: crate::http::types::Method::Post,
            target: "/".to_string(),
            path: "/".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            params: HashMap::new(),
            body: br#"{"id":1,"name":"test"}"#.to_vec(),
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let item: Item = parse_json(&req).unwrap();
        assert_eq!(item.id, 1);
        assert_eq!(item.name, "test");
    }

    #[test]
    fn test_json_parse_error() {
        let req = Request {
            method: crate::http::types::Method::Post,
            target: "/".to_string(),
            path: "/".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            params: HashMap::new(),
            body: b"not json".to_vec(),
            request_id: "test".to_string(),
            remote_addr: None,
        };
        assert!(parse_json::<Item>(&req).is_err());
    }
}

use crate::error::{NanowebError, Result};
use crate::http::parser::{Method, RawHttpRequest};
use serde::de::DeserializeOwned;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub remote_addr: Option<String>,
    pub request_id: String,
}

impl Request {
    pub fn from_raw(raw: RawHttpRequest, remote_addr: Option<String>) -> Self {
        let (path, query) = parse_path_and_query(&raw.request_line.path);
        Request {
            method: raw.request_line.method,
            path,
            version: raw.request_line.version,
            headers: raw.headers,
            body: raw.body,
            params: HashMap::new(),
            query,
            remote_addr,
            request_id: generate_request_id(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(|s| s.as_str())
    }

    pub fn content_type(&self) -> Option<&str> {
        self.header("content-type")
    }

    pub fn content_length(&self) -> Option<usize> {
        self.header("content-length").and_then(|s| s.parse().ok())
    }

    pub fn is_chunked(&self) -> bool {
        self.header("transfer-encoding") == Some("chunked")
    }

    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T> {
        let ct = self.content_type().unwrap_or("");

        if ct.contains("application/json") {
            serde_json::from_slice(&self.body)
                .map_err(|e| NanowebError::Body(format!("JSON parse error: {e}")))
        } else if ct.contains("application/x-www-form-urlencoded") {
            parse_urlencoded(&self.body)
        } else {
            Err(NanowebError::Body(format!(
                "unsupported content type: {ct}"
            )))
        }
    }

    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }

    pub fn body_string(&self) -> Result<String> {
        String::from_utf8(self.body.clone())
            .map_err(|e| NanowebError::Body(format!("invalid UTF-8: {e}")))
    }

    pub fn accept_encoding(&self) -> Vec<String> {
        self.header("accept-encoding")
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
            .unwrap_or_default()
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        self.header("cookie").and_then(|cookies| {
            cookies.split(';').find_map(|c| {
                let c = c.trim();
                if let Some((k, v)) = c.split_once('=') {
                    if k.trim() == name {
                        return Some(v.trim().to_string());
                    }
                }
                None
            })
        })
    }
}

fn parse_path_and_query(raw_path: &str) -> (String, HashMap<String, String>) {
    if let Some((path, query_str)) = raw_path.split_once('?') {
        let query = parse_query_string(query_str);
        (path.to_string(), query)
    } else {
        (raw_path.to_string(), HashMap::new())
    }
}

fn parse_query_string(query: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            let key = percent_encoding::percent_decode_str(key)
                .decode_utf8_lossy()
                .to_string();
            let value = percent_encoding::percent_decode_str(value)
                .decode_utf8_lossy()
                .to_string();
            params.insert(key, value);
        }
    }
    params
}

fn parse_urlencoded<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    let s =
        std::str::from_utf8(body).map_err(|e| NanowebError::Body(format!("invalid UTF-8: {e}")))?;
    let map: HashMap<String, String> = parse_query_string(s);
    let json_val = serde_json::to_value(&map)
        .map_err(|e| NanowebError::Body(format!("serialization error: {e}")))?;
    serde_json::from_value(json_val)
        .map_err(|e| NanowebError::Body(format!("deserialization error: {e}")))
}

fn generate_request_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD_NO_PAD, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_query() {
        let (path, query) = parse_path_and_query("/search?q=hello&page=2");
        assert_eq!(path, "/search");
        assert_eq!(query.get("q").unwrap(), "hello");
        assert_eq!(query.get("page").unwrap(), "2");
    }

    #[test]
    fn test_request_id_unique() {
        let r1 = generate_request_id();
        let r2 = generate_request_id();
        assert_ne!(r1, r2);
    }
}

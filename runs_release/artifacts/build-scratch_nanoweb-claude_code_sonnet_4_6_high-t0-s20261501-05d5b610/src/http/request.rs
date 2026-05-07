use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::net::SocketAddr;

use crate::error::NanowebError;
use crate::http::types::{Headers, Method, Version};

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub query: String,
    pub version: Version,
    pub headers: Headers,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub remote_addr: Option<SocketAddr>,
    pub request_id: String,
    pub extensions: Extensions,
}

#[derive(Debug, Clone, Default)]
pub struct Extensions(HashMap<String, String>);

impl Extensions {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.as_str())
    }
}

impl Request {
    pub fn new(
        method: Method,
        target: String,
        version: Version,
        headers: Headers,
        body: Vec<u8>,
        remote_addr: Option<SocketAddr>,
        request_id: String,
    ) -> Self {
        let (path, query) = split_target(&target);
        Self {
            method,
            path,
            query,
            version,
            headers,
            body,
            params: HashMap::new(),
            remote_addr,
            request_id,
            extensions: Extensions::new(),
        }
    }

    pub fn client_ip(&self) -> Option<std::net::IpAddr> {
        // Check X-Forwarded-For first
        if let Some(xff) = self.headers.get("x-forwarded-for") {
            if let Some(first) = xff.split(',').next() {
                if let Ok(ip) = first.trim().parse() {
                    return Some(ip);
                }
            }
        }
        self.remote_addr.map(|a| a.ip())
    }

    pub fn content_type(&self) -> Option<&str> {
        self.headers.get("content-type")
    }

    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }

    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T, NanowebError> {
        let ct = self.content_type().unwrap_or("").to_lowercase();

        if ct.contains("application/json") {
            serde_json::from_slice(&self.body).map_err(NanowebError::Json)
        } else if ct.contains("application/x-www-form-urlencoded") {
            serde_urlencoded::from_bytes(&self.body).map_err(|e| NanowebError::Parse(e.to_string()))
        } else if ct.is_empty() && !self.body.is_empty() {
            // Try JSON as fallback
            serde_json::from_slice(&self.body).map_err(NanowebError::Json)
        } else {
            Err(NanowebError::InvalidContentType(ct.to_string()))
        }
    }

    pub fn body_text(&self) -> Result<&str, NanowebError> {
        std::str::from_utf8(&self.body)
            .map_err(|_| NanowebError::BadRequest("Body is not valid UTF-8".into()))
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        parse_query_param(&self.query, name)
    }

    pub fn query_params(&self) -> HashMap<String, String> {
        parse_all_query_params(&self.query)
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        let cookie_header = self.headers.get("cookie")?;
        for pair in cookie_header.split(';') {
            let pair = pair.trim();
            if let Some(eq) = pair.find('=') {
                let key = pair[..eq].trim();
                if key == name {
                    return Some(pair[eq + 1..].trim().to_string());
                }
            }
        }
        None
    }

    pub fn cookies(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        if let Some(cookie_header) = self.headers.get("cookie") {
            for pair in cookie_header.split(';') {
                let pair = pair.trim();
                if let Some(eq) = pair.find('=') {
                    let key = pair[..eq].trim().to_string();
                    let value = pair[eq + 1..].trim().to_string();
                    map.insert(key, value);
                }
            }
        }
        map
    }
}

fn split_target(target: &str) -> (String, String) {
    if let Some(q) = target.find('?') {
        (target[..q].to_string(), target[q + 1..].to_string())
    } else {
        (target.to_string(), String::new())
    }
}

fn parse_query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    for pair in query.split('&') {
        if let Some(eq) = pair.find('=') {
            if &pair[..eq] == name {
                return Some(&pair[eq + 1..]);
            }
        }
    }
    None
}

fn parse_all_query_params(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        if let Some(eq) = pair.find('=') {
            let key = percent_decode(&pair[..eq]);
            let value = percent_decode(&pair[eq + 1..]);
            map.insert(key, value);
        } else {
            map.insert(percent_decode(pair), String::new());
        }
    }
    map
}

fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                result.push((h * 16 + l) as char);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            result.push(' ');
            i += 1;
            continue;
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_req(path: &str) -> Request {
        Request::new(
            Method::Get,
            path.to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "test-id".to_string(),
        )
    }

    #[test]
    fn test_path_query_split() {
        let req = make_req("/search?q=hello&page=2");
        assert_eq!(req.path, "/search");
        assert_eq!(req.query, "q=hello&page=2");
        assert_eq!(req.query_param("q"), Some("hello"));
        assert_eq!(req.query_param("page"), Some("2"));
    }

    #[test]
    fn test_cookie_parsing() {
        let mut req = make_req("/");
        req.headers.insert("Cookie", "session=abc; theme=dark");
        assert_eq!(req.cookie("session"), Some("abc".into()));
        assert_eq!(req.cookie("theme"), Some("dark".into()));
        assert_eq!(req.cookie("missing"), None);
    }

    #[test]
    fn test_body_as_json() {
        let mut req = make_req("/");
        req.method = Method::Post;
        req.headers.insert("Content-Type", "application/json");
        req.body = br#"{"name":"alice","age":30}"#.to_vec();

        #[derive(serde::Deserialize)]
        struct User {
            name: String,
            age: u32,
        }
        let user: User = req.body_as().unwrap();
        assert_eq!(user.name, "alice");
        assert_eq!(user.age, 30);
    }

    #[test]
    fn test_body_as_urlencoded() {
        let mut req = make_req("/");
        req.method = Method::Post;
        req.headers
            .insert("Content-Type", "application/x-www-form-urlencoded");
        req.body = b"name=alice&age=30".to_vec();

        #[derive(serde::Deserialize)]
        struct Form {
            name: String,
            age: String,
        }
        let form: Form = req.body_as().unwrap();
        assert_eq!(form.name, "alice");
    }
}

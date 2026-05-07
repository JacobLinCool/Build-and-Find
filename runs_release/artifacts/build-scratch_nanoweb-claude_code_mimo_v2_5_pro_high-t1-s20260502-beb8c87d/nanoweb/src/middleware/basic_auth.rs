use super::Middleware;
use crate::http::types::{Request, Response};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;

pub struct BasicAuthMiddleware {
    username: String,
    password: String,
    realm: String,
}

impl BasicAuthMiddleware {
    pub fn new(username: String, password: String) -> Self {
        Self {
            username,
            password,
            realm: "Restricted".to_string(),
        }
    }
}

impl Middleware for BasicAuthMiddleware {
    fn name(&self) -> &str {
        "basic_auth"
    }

    fn before(&self, req: &mut Request) -> Option<Response> {
        let auth = req.header("authorization");
        match auth {
            Some(val) if val.starts_with("Basic ") => {
                let encoded = &val[6..];
                if let Ok(decoded) = STANDARD.decode(encoded) {
                    if let Ok(credentials) = std::str::from_utf8(&decoded) {
                        let parts: Vec<&str> = credentials.splitn(2, ':').collect();
                        if parts.len() == 2
                            && parts[0] == self.username
                            && parts[1] == self.password
                        {
                            return None;
                        }
                    }
                }
                let mut resp = Response::text(401, "Unauthorized");
                resp.set_header(
                    "www-authenticate",
                    format!("Basic realm=\"{}\"", self.realm),
                );
                Some(resp)
            }
            _ => {
                let mut resp = Response::text(401, "Unauthorized");
                resp.set_header(
                    "www-authenticate",
                    format!("Basic realm=\"{}\"", self.realm),
                );
                Some(resp)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_basic_auth_blocks_without_header() {
        let mw = BasicAuthMiddleware::new("admin".to_string(), "secret".to_string());
        let mut req = Request {
            method: crate::http::types::Method::Get,
            target: "/admin".to_string(),
            path: "/admin".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            params: HashMap::new(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let resp = mw.before(&mut req).unwrap();
        assert_eq!(resp.status, 401);
    }

    #[test]
    fn test_basic_auth_allows_valid_credentials() {
        let mw = BasicAuthMiddleware::new("admin".to_string(), "secret".to_string());
        let encoded = STANDARD.encode(b"admin:secret");
        let mut headers = HashMap::new();
        headers.insert("authorization".to_string(), format!("Basic {encoded}"));
        let mut req = Request {
            method: crate::http::types::Method::Get,
            target: "/admin".to_string(),
            path: "/admin".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers,
            params: HashMap::new(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        assert!(mw.before(&mut req).is_none());
    }
}

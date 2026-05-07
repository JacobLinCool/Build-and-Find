use crate::http::types::{Request, Response, StatusCode};
use crate::middleware::Middleware;

pub struct BasicAuth {
    username: String,
    password: String,
    realm: String,
}

impl BasicAuth {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            realm: "nanoweb".into(),
        }
    }

    pub fn with_realm(mut self, realm: impl Into<String>) -> Self {
        self.realm = realm.into();
        self
    }
}

impl Middleware for BasicAuth {
    fn before(&self, req: &mut Request) -> Option<Response> {
        match req.header("authorization") {
            Some(auth) if auth.starts_with("Basic ") => {
                let encoded = &auth[6..];
                use base64::Engine;
                if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                    if let Ok(s) = std::str::from_utf8(&decoded) {
                        if let Some((user, pass)) = s.split_once(':') {
                            if user == self.username && pass == self.password {
                                return None;
                            }
                        }
                    }
                }
                unauthorized(&self.realm)
            }
            _ => unauthorized(&self.realm),
        }
    }

    fn name(&self) -> &str {
        "basic-auth"
    }
}

fn unauthorized(realm: &str) -> Option<Response> {
    let mut resp = Response::new(StatusCode::UNAUTHORIZED);
    resp.headers
        .insert("www-authenticate", format!("Basic realm=\"{realm}\""));
    Some(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_auth_rejects_no_header() {
        let auth = BasicAuth::new("admin", "secret");
        let mut req = Request {
            method: crate::http::types::Method::Get,
            path: "/".into(),
            query: String::new(),
            version: "HTTP/1.1".into(),
            headers: crate::http::types::Headers::new(),
            body: Vec::new(),
            params: Default::default(),
            request_id: "test".into(),
        };
        assert!(auth.before(&mut req).is_some());
    }

    #[test]
    fn basic_auth_accepts_valid() {
        let auth = BasicAuth::new("admin", "secret");
        let mut req = Request {
            method: crate::http::types::Method::Get,
            path: "/".into(),
            query: String::new(),
            version: "HTTP/1.1".into(),
            headers: crate::http::types::Headers::new(),
            body: Vec::new(),
            params: Default::default(),
            request_id: "test".into(),
        };
        use base64::Engine;
        let cred = base64::engine::general_purpose::STANDARD.encode(b"admin:secret");
        req.headers.insert("authorization", format!("Basic {cred}"));
        assert!(auth.before(&mut req).is_none());
    }
}

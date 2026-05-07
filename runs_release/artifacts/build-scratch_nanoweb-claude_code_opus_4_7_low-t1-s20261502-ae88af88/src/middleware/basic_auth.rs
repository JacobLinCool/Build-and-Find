use super::Middleware;
use crate::http::{Request, Response, StatusCode};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};

pub struct BasicAuth {
    pub username: String,
    pub password: String,
    pub realm: String,
}

impl BasicAuth {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            realm: "nanoweb".into(),
        }
    }
}

#[async_trait]
impl Middleware for BasicAuth {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        let header = req.header("Authorization").unwrap_or("");
        if let Some(b64) = header.strip_prefix("Basic ") {
            if let Ok(bytes) = STANDARD.decode(b64.trim()) {
                if let Ok(s) = std::str::from_utf8(&bytes) {
                    if let Some((u, p)) = s.split_once(':') {
                        if u == self.username && p == self.password {
                            req.extensions.set("auth_user", u);
                            return None;
                        }
                    }
                }
            }
        }
        let mut r = Response::new(StatusCode::UNAUTHORIZED);
        r.headers.insert(
            "WWW-Authenticate",
            format!("Basic realm=\"{}\"", self.realm),
        );
        r.body = b"unauthorized".to_vec();
        Some(r)
    }
}

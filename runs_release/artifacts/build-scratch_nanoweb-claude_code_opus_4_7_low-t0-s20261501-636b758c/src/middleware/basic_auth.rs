use super::Middleware;
use crate::http::{Request, Response, StatusCode};
use crate::NanowebError;
use async_trait::async_trait;
use base64::Engine;
use std::collections::HashMap;

pub struct BasicAuth {
    pub realm: String,
    pub users: HashMap<String, String>,
}

impl BasicAuth {
    pub fn new(realm: impl Into<String>, users: HashMap<String, String>) -> Self {
        Self {
            realm: realm.into(),
            users,
        }
    }
}

#[async_trait]
impl Middleware for BasicAuth {
    async fn before(&self, req: &mut Request) -> Result<Option<Response>, NanowebError> {
        let authz = req.header("authorization").unwrap_or("");
        if let Some(rest) = authz.strip_prefix("Basic ") {
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(rest.trim()) {
                if let Ok(s) = std::str::from_utf8(&bytes) {
                    if let Some((user, pass)) = s.split_once(':') {
                        if self.users.get(user).map(|p| p == pass).unwrap_or(false) {
                            req.extensions.user = Some(user.to_string());
                            return Ok(None);
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
        Ok(Some(r))
    }
}

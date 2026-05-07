//! HTTP basic-auth guard.

use crate::error::Result;
use crate::http::{Request, Response, StatusCode};

use super::{BoxFuture, Middleware, Next};

pub struct BasicAuthMiddleware {
    pub realm: String,
    pub credentials: Vec<(String, String)>,
}

impl BasicAuthMiddleware {
    pub fn new(realm: impl Into<String>, credentials: Vec<(String, String)>) -> Self {
        Self {
            realm: realm.into(),
            credentials,
        }
    }

    fn matches(&self, user: &str, pass: &str) -> bool {
        self.credentials
            .iter()
            .any(|(u, p)| u == user && constant_time_eq(p.as_bytes(), pass.as_bytes()))
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

impl Middleware for BasicAuthMiddleware {
    fn handle<'a>(
        &'a self,
        req: &'a mut Request,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            use base64::Engine;
            let header = req.header("authorization");
            let creds = header
                .and_then(|h| {
                    h.strip_prefix("Basic ")
                        .or_else(|| h.strip_prefix("basic "))
                })
                .and_then(|enc| {
                    base64::engine::general_purpose::STANDARD
                        .decode(enc.trim())
                        .ok()
                })
                .and_then(|bytes| String::from_utf8(bytes).ok());
            if let Some(decoded) = creds {
                if let Some((u, p)) = decoded.split_once(':') {
                    if self.matches(u, p) {
                        req.extensions.insert("auth_user".into(), u.into());
                        return next.run(req).await;
                    }
                }
            }
            Ok(Response::new(StatusCode::UNAUTHORIZED)
                .header(
                    "www-authenticate",
                    format!("Basic realm=\"{}\"", self.realm),
                )
                .text("authentication required"))
        })
    }
}

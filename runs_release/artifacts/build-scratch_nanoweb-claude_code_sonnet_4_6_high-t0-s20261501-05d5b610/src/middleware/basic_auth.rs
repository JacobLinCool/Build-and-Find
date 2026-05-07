use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;

use crate::error::NanowebError;
use crate::http::{Request, Response};
use crate::middleware::{Middleware, Next};

pub struct BasicAuthMiddleware {
    pub realm: String,
    /// Validator function: (username, password) -> bool
    validator: Box<dyn Fn(&str, &str) -> bool + Send + Sync>,
}

impl BasicAuthMiddleware {
    pub fn new(
        realm: impl Into<String>,
        validator: impl Fn(&str, &str) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            realm: realm.into(),
            validator: Box::new(validator),
        }
    }

    /// Simple username/password credential check.
    pub fn with_credentials(realm: impl Into<String>, username: String, password: String) -> Self {
        Self::new(realm, move |u, p| u == username && p == password)
    }
}

#[async_trait::async_trait]
impl Middleware for BasicAuthMiddleware {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
        let auth_header = req.headers.get("authorization").map(|s| s.to_string());

        match auth_header {
            Some(ref h) if h.to_lowercase().starts_with("basic ") => {
                let encoded = &h[6..];
                match STANDARD.decode(encoded) {
                    Ok(decoded) => {
                        let s = String::from_utf8_lossy(&decoded);
                        if let Some(colon) = s.find(':') {
                            let username = &s[..colon];
                            let password = &s[colon + 1..];
                            if (self.validator)(username, password) {
                                return next.run(req).await;
                            }
                        }
                    }
                    Err(_) => {}
                }
                unauthorized_response(&self.realm)
            }
            _ => unauthorized_response(&self.realm),
        }
    }
}

fn unauthorized_response(realm: &str) -> Result<Response, NanowebError> {
    let mut resp = Response::new(401);
    resp.headers
        .insert("WWW-Authenticate", format!("Basic realm=\"{}\"", realm));
    resp.body = b"Unauthorized".to_vec();
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Headers, Method, Version};
    use crate::middleware::MiddlewareChain;
    use std::sync::Arc;

    fn make_req_with_auth(credentials: &str) -> Request {
        let encoded = STANDARD.encode(credentials);
        let mut req = Request::new(
            Method::Get,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        req.headers
            .insert("Authorization", format!("Basic {}", encoded));
        req
    }

    #[tokio::test]
    async fn test_valid_credentials() {
        let mw = Arc::new(BasicAuthMiddleware::with_credentials(
            "Test",
            "admin".to_string(),
            "secret".to_string(),
        ));
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok()) })
        });
        let chain = MiddlewareChain::new(vec![mw], handler);
        let resp = chain.run(make_req_with_auth("admin:secret")).await.unwrap();
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn test_invalid_credentials() {
        let mw = Arc::new(BasicAuthMiddleware::with_credentials(
            "Test",
            "admin".to_string(),
            "secret".to_string(),
        ));
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok()) })
        });
        let chain = MiddlewareChain::new(vec![mw], handler);
        let resp = chain.run(make_req_with_auth("admin:wrong")).await.unwrap();
        assert_eq!(resp.status, 401);
    }

    #[tokio::test]
    async fn test_missing_credentials() {
        let mw = Arc::new(BasicAuthMiddleware::with_credentials(
            "Test",
            "admin".to_string(),
            "secret".to_string(),
        ));
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok()) })
        });
        let chain = MiddlewareChain::new(vec![mw], handler);
        let req = Request::new(
            Method::Get,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        let resp = chain.run(req).await.unwrap();
        assert_eq!(resp.status, 401);
        assert!(resp.headers.get("www-authenticate").is_some());
    }
}

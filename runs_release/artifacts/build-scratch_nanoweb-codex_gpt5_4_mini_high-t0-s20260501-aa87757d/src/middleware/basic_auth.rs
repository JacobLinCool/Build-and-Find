use crate::{
    app::AppState,
    error::Result,
    http::types::{Request, Response, StatusCode},
    middleware::{Middleware, Next},
};
use async_trait::async_trait;
use base64::Engine;
use std::sync::Arc;

pub struct BasicAuthMiddleware {
    username: String,
    password: String,
}

impl BasicAuthMiddleware {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }
}

#[async_trait]
impl Middleware for BasicAuthMiddleware {
    async fn handle(&self, req: Request, ctx: Arc<AppState>, next: Next<'_>) -> Result<Response> {
        let auth = req.header("authorization").unwrap_or("");
        let expected = format!("{}:{}", self.username, self.password);
        let authorized = auth
            .strip_prefix("Basic ")
            .and_then(|encoded| {
                base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .ok()
            })
            .and_then(|decoded| String::from_utf8(decoded).ok())
            .map(|value| value == expected)
            .unwrap_or(false);
        if !authorized {
            let mut response = Response::text(StatusCode::Unauthorized, "authentication required");
            response.set_header("www-authenticate", "Basic realm=\"nanoweb\"");
            return Ok(response);
        }
        let _ = ctx;
        next.run(req).await
    }
}

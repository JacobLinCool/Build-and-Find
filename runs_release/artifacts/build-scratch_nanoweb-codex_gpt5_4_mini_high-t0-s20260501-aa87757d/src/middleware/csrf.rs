use crate::{
    app::AppState,
    error::Result,
    http::types::{Request, Response, StatusCode},
    middleware::{Middleware, Next},
};
use async_trait::async_trait;
use base64::Engine;
use rand::{rngs::OsRng, RngCore};
use std::sync::Arc;

pub struct CsrfMiddleware {
    cookie_name: String,
    header_name: String,
}

impl CsrfMiddleware {
    pub fn new(cookie_name: impl Into<String>, header_name: impl Into<String>) -> Self {
        Self {
            cookie_name: cookie_name.into(),
            header_name: header_name.into(),
        }
    }

    fn read_cookie(&self, request: &Request) -> Option<String> {
        let cookies = request.header("cookie")?;
        for pair in cookies.split(';') {
            let (name, value) = pair.trim().split_once('=')?;
            if name.trim() == self.cookie_name {
                return Some(value.trim().to_string());
            }
        }
        None
    }
}

#[async_trait]
impl Middleware for CsrfMiddleware {
    async fn handle(&self, req: Request, ctx: Arc<AppState>, next: Next<'_>) -> Result<Response> {
        let cookie = self.read_cookie(&req);
        if !req.method.is_safe() {
            let header = req.header(&self.header_name).map(|value| value.to_string());
            if cookie.is_none() || header.is_none() || cookie != header {
                return Ok(Response::text(
                    StatusCode::Forbidden,
                    "csrf validation failed",
                ));
            }
        }
        let mut response = next.run(req).await?;
        if cookie.is_none() {
            let mut bytes = [0u8; 16];
            OsRng.fill_bytes(&mut bytes);
            let token = base64::engine::general_purpose::STANDARD.encode(bytes);
            response.set_header(
                "set-cookie",
                format!(
                    "{}={}; HttpOnly; Path=/; SameSite=Lax",
                    self.cookie_name, token
                ),
            );
        }
        let _ = ctx;
        Ok(response)
    }
}

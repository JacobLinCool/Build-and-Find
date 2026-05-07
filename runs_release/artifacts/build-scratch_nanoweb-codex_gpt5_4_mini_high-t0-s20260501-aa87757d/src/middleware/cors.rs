use crate::{
    app::AppState,
    error::Result,
    http::types::{Request, Response},
    middleware::{Middleware, Next},
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct CorsMiddleware {
    allowed_origins: Vec<String>,
}

impl CorsMiddleware {
    pub fn new(allowed_origins: Vec<String>) -> Self {
        Self { allowed_origins }
    }
}

#[async_trait]
impl Middleware for CorsMiddleware {
    async fn handle(&self, req: Request, ctx: Arc<AppState>, next: Next<'_>) -> Result<Response> {
        let origin = req.header("origin").map(|value| value.to_string());
        let mut response = next.run(req.clone()).await?;
        if let Some(origin) = origin {
            if self
                .allowed_origins
                .iter()
                .any(|candidate| candidate == "*" || candidate == &origin)
            {
                response.set_header("access-control-allow-origin", origin);
                response.set_header("vary", "Origin");
                response.set_header("access-control-allow-credentials", "true");
            }
        }
        let _ = ctx;
        Ok(response)
    }
}

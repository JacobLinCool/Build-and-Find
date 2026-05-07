use crate::{
    app::AppState,
    error::Result,
    http::types::{Request, Response, StatusCode},
    middleware::Handler,
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct HealthHandler;
pub struct ReadyHandler;

#[async_trait]
impl Handler for HealthHandler {
    async fn handle(&self, _req: Request, _ctx: Arc<AppState>) -> Result<Response> {
        Ok(Response::text(StatusCode::Ok, "ok"))
    }
}

#[async_trait]
impl Handler for ReadyHandler {
    async fn handle(&self, _req: Request, ctx: Arc<AppState>) -> Result<Response> {
        if ctx.ready.load(std::sync::atomic::Ordering::SeqCst) {
            Ok(Response::text(StatusCode::Ok, "ready"))
        } else {
            Ok(Response::text(StatusCode::ServiceUnavailable, "not ready"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HealthHandler;
    use crate::middleware::Handler;
    use crate::{
        app::AppState,
        http::types::{Method, Request, Version},
    };
    use std::{collections::HashMap, sync::Arc};

    #[tokio::test]
    async fn health_ok() {
        let state = Arc::new(AppState::test());
        let req = Request {
            method: Method::GET,
            target: "/healthz".to_string(),
            path: "/healthz".to_string(),
            query: None,
            version: Version::Http11,
            headers: HashMap::new(),
            body: Vec::new(),
            params: HashMap::new(),
            request_id: "req".to_string(),
            remote_addr: None,
            route_pattern: None,
            connection_close: false,
        };
        let response = HealthHandler.handle(req, state).await.unwrap();
        assert_eq!(response.status.as_u16(), 200);
    }
}

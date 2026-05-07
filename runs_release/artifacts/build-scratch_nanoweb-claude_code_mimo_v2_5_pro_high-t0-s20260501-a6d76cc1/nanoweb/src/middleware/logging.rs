use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use std::time::Instant;

pub struct LoggingMiddleware;

#[async_trait]
impl Middleware for LoggingMiddleware {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        req.headers.insert(
            "_start_time".into(),
            format!("{}", Instant::now().elapsed().as_nanos()),
        );
        None
    }

    async fn after(&self, req: &Request, resp: &mut Response) {
        let method = req.method.as_str();
        let path = &req.path;
        let status = resp.status;
        let status_class = match status {
            200..=299 => "2xx",
            300..=399 => "3xx",
            400..=499 => "4xx",
            500..=599 => "5xx",
            _ => "other",
        };
        tracing::info!(
            method = method,
            path = path,
            status = status,
            status_class = status_class,
            request_id = req.request_id.as_str(),
            "request completed"
        );
    }
}

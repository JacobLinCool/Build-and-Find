use super::Middleware;
use crate::http::types::{Request, Response};
use tracing::info;

pub struct LoggingMiddleware;

impl LoggingMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Middleware for LoggingMiddleware {
    fn name(&self) -> &str {
        "logging"
    }

    fn after(&self, req: &Request, resp: &mut Response) {
        resp.set_header("x-request-id", &req.request_id);
        info!(
            method = %req.method,
            path = %req.path,
            status = resp.status,
            request_id = %req.request_id,
            remote_addr = req.remote_addr.as_deref().unwrap_or("unknown"),
            "request completed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_logging_sets_request_id() {
        let mw = LoggingMiddleware::new();
        let req = Request {
            method: crate::http::types::Method::Get,
            target: "/test".to_string(),
            path: "/test".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            params: HashMap::new(),
            body: vec![],
            request_id: "req-123".to_string(),
            remote_addr: Some("127.0.0.1:12345".to_string()),
        };
        let mut resp = Response::ok();
        mw.after(&req, &mut resp);
        assert_eq!(resp.header("x-request-id").unwrap(), "req-123");
    }
}

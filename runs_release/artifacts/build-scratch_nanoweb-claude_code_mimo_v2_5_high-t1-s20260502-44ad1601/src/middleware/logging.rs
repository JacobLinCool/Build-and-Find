use crate::http::request::Request;
use crate::http::response::Response;
use crate::middleware::Middleware;
use std::time::Instant;

pub struct LoggingMiddleware;

impl LoggingMiddleware {
    pub fn new() -> Self {
        LoggingMiddleware
    }
}

impl Default for LoggingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for LoggingMiddleware {
    fn handle(&self, req: Request, next: Box<dyn FnOnce(Request) -> Response + Send>) -> Response {
        let method = req.method.as_str().to_string();
        let path = req.path.clone();
        let remote = req.remote_addr.clone().unwrap_or_default();
        let start = Instant::now();

        let resp = next(req);

        let duration = start.elapsed().as_secs_f64();
        eprintln!(
            "[nanoweb] {} {} {} {} {:.6}s",
            remote, method, path, resp.status, duration
        );

        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_middleware() {
        let mw = LoggingMiddleware::new();
        let req = Request {
            method: crate::http::parser::Method::Get,
            path: "/test".into(),
            version: "HTTP/1.1".into(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            query: std::collections::HashMap::new(),
            remote_addr: Some("127.0.0.1:12345".into()),
            request_id: "test-id".into(),
        };
        let resp = mw.handle(req, Box::new(|_| Response::text("ok")));
        assert_eq!(resp.status, 200);
    }
}

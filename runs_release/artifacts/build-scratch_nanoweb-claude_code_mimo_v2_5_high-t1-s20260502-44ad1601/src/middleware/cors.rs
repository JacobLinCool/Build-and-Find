use crate::http::request::Request;
use crate::http::response::Response;
use crate::middleware::Middleware;

pub struct CorsMiddleware {
    allowed_origins: Vec<String>,
}

impl CorsMiddleware {
    pub fn new(allowed_origins: Vec<String>) -> Self {
        CorsMiddleware { allowed_origins }
    }
}

impl Middleware for CorsMiddleware {
    fn handle(&self, req: Request, next: Box<dyn FnOnce(Request) -> Response + Send>) -> Response {
        let origin = req.header("origin").unwrap_or("").to_string();
        let is_preflight = req.method == crate::http::parser::Method::Options;

        if is_preflight {
            let mut resp = Response::new(204);
            if self.allowed_origins.contains(&"*".to_string())
                || self.allowed_origins.contains(&origin)
            {
                resp.headers
                    .insert("access-control-allow-origin".into(), origin.clone());
            }
            resp.headers.insert(
                "access-control-allow-methods".into(),
                "GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS".into(),
            );
            resp.headers.insert(
                "access-control-allow-headers".into(),
                "Content-Type, Authorization".into(),
            );
            resp.headers
                .insert("access-control-max-age".into(), "86400".into());
            return resp;
        }

        let mut resp = next(req);

        if self.allowed_origins.contains(&"*".to_string()) || self.allowed_origins.contains(&origin)
        {
            resp.headers
                .insert("access-control-allow-origin".into(), origin);
        }

        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cors_preflight() {
        let mw = CorsMiddleware::new(vec!["*".into()]);
        let mut req = Request {
            method: crate::http::parser::Method::Options,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            query: std::collections::HashMap::new(),
            remote_addr: None,
            request_id: "test".into(),
        };
        req.headers
            .insert("origin".into(), "http://example.com".into());
        let resp = mw.handle(req, Box::new(|_| Response::text("ok")));
        assert_eq!(resp.status, 204);
        assert!(resp.headers.contains_key("access-control-allow-methods"));
    }
}

use crate::config::CorsConfig;
use crate::http::types::{Request, Response};
use crate::middleware::Middleware;

pub struct Cors {
    config: CorsConfig,
}

impl Cors {
    pub fn new(config: CorsConfig) -> Self {
        Self { config }
    }
}

impl Middleware for Cors {
    fn before(&self, req: &mut Request) -> Option<Response> {
        // Handle preflight OPTIONS
        if req.method == crate::http::types::Method::Options {
            let origin = req.header("origin").unwrap_or("*");
            let mut resp = Response::ok();
            resp.headers
                .insert("access-control-allow-origin", self.allow_origin(origin));
            resp.headers.insert(
                "access-control-allow-methods",
                self.config.allowed_methods.join(", "),
            );
            resp.headers.insert(
                "access-control-allow-headers",
                self.config.allowed_headers.join(", "),
            );
            resp.headers
                .insert("access-control-max-age", self.config.max_age.to_string());
            return Some(resp);
        }
        None
    }

    fn after(&self, req: &Request, mut resp: Response) -> Response {
        let origin = req.header("origin").unwrap_or("*");
        resp.headers
            .insert("access-control-allow-origin", self.allow_origin(origin));
        resp
    }

    fn name(&self) -> &str {
        "cors"
    }
}

impl Cors {
    fn allow_origin(&self, origin: &str) -> String {
        if self.config.allowed_origins.iter().any(|o| o == "*") {
            return "*".into();
        }
        if self.config.allowed_origins.iter().any(|o| o == origin) {
            return origin.to_string();
        }
        self.config
            .allowed_origins
            .first()
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cors_allows_wildcard() {
        let cors = Cors::new(CorsConfig {
            allowed_origins: vec!["*".into()],
            ..Default::default()
        });
        let mut req = Request {
            method: crate::http::types::Method::Get,
            path: "/".into(),
            query: String::new(),
            version: "HTTP/1.1".into(),
            headers: crate::http::types::Headers::new(),
            body: Vec::new(),
            params: Default::default(),
            request_id: "test".into(),
        };
        req.headers.insert("origin", "http://example.com");
        let resp = cors.after(&req, Response::ok());
        assert_eq!(resp.headers.get("access-control-allow-origin"), Some("*"));
    }
}

//! Cross-Origin Resource Sharing middleware.
//!
//! * Responds to `OPTIONS` preflight requests directly with the
//!   configured allow headers.
//! * Attaches `Access-Control-Allow-*` headers to regular responses.

use std::future::Future;
use std::pin::Pin;

use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response, StatusCode};
use crate::middleware::{Middleware, MiddlewareOutcome};

#[derive(Clone)]
pub struct CorsConfig {
    pub allow_origin: String,
    pub allow_methods: Vec<Method>,
    pub allow_headers: Vec<String>,
    pub allow_credentials: bool,
    pub max_age_seconds: u32,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allow_origin: "*".to_string(),
            allow_methods: vec![Method::Get, Method::Post, Method::Put, Method::Delete],
            allow_headers: vec![
                "content-type".into(),
                "authorization".into(),
                "x-request-id".into(),
            ],
            allow_credentials: false,
            max_age_seconds: 600,
        }
    }
}

pub struct CorsMiddleware {
    pub config: CorsConfig,
}

impl CorsMiddleware {
    pub fn new(config: CorsConfig) -> Self {
        Self { config }
    }

    fn decorate(&self, resp: &mut Response) {
        resp.headers
            .set("access-control-allow-origin", &self.config.allow_origin);
        if self.config.allow_credentials {
            resp.headers.set("access-control-allow-credentials", "true");
        }
        let methods = self
            .config
            .allow_methods
            .iter()
            .map(|m| m.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        resp.headers.set("access-control-allow-methods", methods);
        resp.headers.set(
            "access-control-allow-headers",
            self.config.allow_headers.join(", "),
        );
        resp.headers.set(
            "access-control-max-age",
            self.config.max_age_seconds.to_string(),
        );
    }
}

impl Default for CorsMiddleware {
    fn default() -> Self {
        Self::new(CorsConfig::default())
    }
}

impl Middleware for CorsMiddleware {
    fn name(&self) -> &'static str {
        "cors"
    }

    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<MiddlewareOutcome, NanowebError>> + Send + 'a>>
    {
        Box::pin(async move {
            let is_preflight = req.method == Method::Options
                && req.headers.get("access-control-request-method").is_some();
            if is_preflight {
                let mut resp = Response::status(StatusCode::NO_CONTENT);
                self.decorate(&mut resp);
                return Ok(MiddlewareOutcome::Respond(resp));
            }
            Ok(MiddlewareOutcome::Continue(req))
        })
    }

    fn after<'a>(
        &'a self,
        _req: &'a Request,
        mut resp: Response,
    ) -> Pin<Box<dyn Future<Output = Result<Response, NanowebError>> + Send + 'a>> {
        Box::pin(async move {
            self.decorate(&mut resp);
            Ok(resp)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn adds_allow_origin_header() {
        let m = CorsMiddleware::default();
        let req = Request::new(Method::Get, "/x");
        let resp = Response::status(StatusCode::OK);
        let out = m.after(&req, resp).await.unwrap();
        assert_eq!(out.headers.get("access-control-allow-origin"), Some("*"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn preflight_short_circuits() {
        let m = CorsMiddleware::default();
        let mut req = Request::new(Method::Options, "/x");
        req.headers.set("access-control-request-method", "POST");
        match m.before(req).await.unwrap() {
            MiddlewareOutcome::Respond(resp) => {
                assert_eq!(resp.status, StatusCode::NO_CONTENT);
                assert!(resp.headers.get("access-control-allow-methods").is_some());
            }
            _ => panic!("expected preflight short-circuit"),
        }
    }
}

pub mod basic_auth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limit;

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::Result;
use crate::http::{Request, Response};
use crate::router::{Lookup, Router};

#[async_trait]
pub trait Middleware: Send + Sync + 'static {
    async fn before(&self, _request: &mut Request) -> Result<Option<Response>> {
        Ok(None)
    }

    async fn after(&self, _request: &Request, _response: &mut Response) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct MiddlewareStack {
    middleware: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push<M: Middleware>(&mut self, middleware: M) {
        self.middleware.push(Arc::new(middleware));
    }

    pub async fn handle(&self, router: &Router, mut request: Request) -> Result<Response> {
        let snapshot = request.clone();
        for middleware in &self.middleware {
            if let Some(mut response) = middleware.before(&mut request).await? {
                for middleware in self.middleware.iter().rev() {
                    middleware.after(&snapshot, &mut response).await?;
                }
                response.set_header("X-Request-Id", snapshot.request_id.clone());
                return Ok(response);
            }
        }

        if let Lookup::Found(found) = router.lookup(&request.method, &request.path) {
            request.route_pattern = Some(found.pattern);
            request.params = found.params;
        }
        let final_request = request.clone();
        let mut response = router.handle(request).await?;
        for middleware in self.middleware.iter().rev() {
            middleware.after(&final_request, &mut response).await?;
        }
        response.set_header("X-Request-Id", final_request.request_id);
        Ok(response)
    }
}

pub(crate) fn cookie_value(header: Option<&str>, name: &str) -> Option<String> {
    header?.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name).then(|| value.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, StatusCode};

    struct HeaderMiddleware;

    #[async_trait]
    impl Middleware for HeaderMiddleware {
        async fn after(&self, _request: &Request, response: &mut Response) -> Result<()> {
            response.set_header("X-Test", "1");
            Ok(())
        }
    }

    #[tokio::test]
    async fn runs_after_middleware() {
        let mut router = Router::new();
        router.add(Method::Get, "/", |_req: Request| async {
            Ok(Response::text(StatusCode::OK, "ok"))
        });
        let mut stack = MiddlewareStack::new();
        stack.push(HeaderMiddleware);
        let res = stack
            .handle(&router, Request::new(Method::Get, "/"))
            .await
            .unwrap();
        assert_eq!(res.header("x-test"), Some("1"));
    }
}

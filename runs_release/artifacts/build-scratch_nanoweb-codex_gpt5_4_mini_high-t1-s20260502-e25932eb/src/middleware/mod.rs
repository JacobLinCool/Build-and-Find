use std::sync::Arc;

use async_trait::async_trait;

use crate::error::Result;
use crate::http::{Request, Response};
use crate::router::Handler;

pub mod basic_auth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limit;

#[async_trait]
pub trait Middleware: Send + Sync {
    async fn handle(&self, request: Request, next: Next<'_>) -> Result<Response>;
}

pub struct Next<'a> {
    middlewares: &'a [Arc<dyn Middleware>],
    handler: &'a Arc<dyn Handler>,
    index: usize,
}

impl<'a> Next<'a> {
    pub async fn run(self, request: Request) -> Result<Response> {
        if self.index >= self.middlewares.len() {
            self.handler.handle(request).await
        } else {
            let middleware = &self.middlewares[self.index];
            middleware
                .handle(
                    request,
                    Next {
                        middlewares: self.middlewares,
                        handler: self.handler,
                        index: self.index + 1,
                    },
                )
                .await
        }
    }
}

pub async fn execute(
    middlewares: &[Arc<dyn Middleware>],
    handler: Arc<dyn Handler>,
    request: Request,
) -> Result<Response> {
    Next {
        middlewares,
        handler: &handler,
        index: 0,
    }
    .run(request)
    .await
}

pub fn boxed<M: Middleware + 'static>(middleware: M) -> Arc<dyn Middleware> {
    Arc::new(middleware)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, StatusCode};
    use crate::router::Handler;

    struct AddHeader;

    #[async_trait]
    impl Middleware for AddHeader {
        async fn handle(&self, request: Request, next: Next<'_>) -> Result<Response> {
            let mut response = next.run(request).await?;
            response.headers.insert("x-added", "yes");
            Ok(response)
        }
    }

    struct OkHandler;

    #[async_trait]
    impl Handler for OkHandler {
        async fn handle(&self, _request: Request) -> Result<Response> {
            Ok(Response::text(StatusCode::Ok, "ok"))
        }
    }

    #[tokio::test]
    async fn executes_in_order() {
        let request = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: crate::http::Version::Http11,
            headers: Default::default(),
            body: vec![],
            params: Default::default(),
            remote_addr: None,
            request_id: "req".into(),
            session: None,
        };
        let response = execute(&[Arc::new(AddHeader)], Arc::new(OkHandler), request)
            .await
            .unwrap();
        assert_eq!(response.headers.get("x-added"), Some("yes"));
    }
}

use crate::http::{Request, Response};
use async_trait::async_trait;
use std::sync::Arc;

pub mod basic_auth;
pub mod compression;
pub mod cors;
pub mod csrf;
pub mod logging;
pub mod rate_limit;

pub use basic_auth::BasicAuth;
pub use compression::Compression;
pub use cors::Cors;
pub use csrf::Csrf;
pub use logging::RequestLogger;
pub use rate_limit::RateLimit;

/// Object-safe middleware trait. `before` may return a short-circuit response;
/// `after` observes / mutates a response produced by the handler or earlier
/// middleware.
#[async_trait]
pub trait Middleware: Send + Sync {
    fn name(&self) -> &'static str;

    async fn before(&self, _req: &mut Request) -> Option<Response> {
        None
    }

    async fn after(&self, _req: &Request, _resp: &mut Response) {}
}

pub type SharedMiddleware = Arc<dyn Middleware>;

/// An ordered chain of middleware. Registration order is preserved.
#[derive(Default, Clone)]
pub struct Chain {
    items: Vec<SharedMiddleware>,
}

impl Chain {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push<M: Middleware + 'static>(mut self, m: M) -> Self {
        self.items.push(Arc::new(m));
        self
    }
    pub fn push_arc(mut self, m: SharedMiddleware) -> Self {
        self.items.push(m);
        self
    }
    pub fn iter(&self) -> impl Iterator<Item = &SharedMiddleware> {
        self.items.iter()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Run the chain around a handler invocation. `before` runs in registration
/// order; `after` runs in reverse, mirroring nesting.
pub async fn run<F, Fut>(chain: &Chain, mut req: Request, handler: F) -> Response
where
    F: FnOnce(Request) -> Fut,
    Fut: std::future::Future<Output = Response>,
{
    // before
    let mut short_circuit: Option<Response> = None;
    let mut ran_to: usize = chain.len();
    for (i, mw) in chain.items.iter().enumerate() {
        if let Some(r) = mw.before(&mut req).await {
            short_circuit = Some(r);
            ran_to = i + 1;
            break;
        }
    }
    let mut resp = if let Some(r) = short_circuit {
        r
    } else {
        handler(req.clone()).await
    };
    // after, in reverse, only for middlewares that ran before
    for mw in chain.items[..ran_to].iter().rev() {
        mw.after(&req, &mut resp).await;
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, Version};

    struct Tag(&'static str);
    #[async_trait]
    impl Middleware for Tag {
        fn name(&self) -> &'static str {
            self.0
        }
        async fn after(&self, _req: &Request, resp: &mut Response) {
            resp.headers.append("X-Trace", self.0);
        }
    }

    struct ShortCircuit;
    #[async_trait]
    impl Middleware for ShortCircuit {
        fn name(&self) -> &'static str {
            "sc"
        }
        async fn before(&self, _req: &mut Request) -> Option<Response> {
            Some(Response::text("blocked"))
        }
    }

    #[tokio::test]
    async fn order_preserved_after_runs_reverse() {
        let chain = Chain::new().push(Tag("a")).push(Tag("b"));
        let req = Request::new(Method::Get, "/".into(), Version::Http11);
        let resp = run(&chain, req, |_r| async { Response::text("ok") }).await;
        let traces: Vec<&str> = resp.headers.get_all("X-Trace");
        // after runs reverse: b first, then a
        assert_eq!(traces, vec!["b", "a"]);
    }

    #[tokio::test]
    async fn short_circuit_skips_handler() {
        let chain = Chain::new().push(ShortCircuit);
        let req = Request::new(Method::Get, "/".into(), Version::Http11);
        let resp = run(&chain, req, |_r| async { Response::text("nope") }).await;
        assert_eq!(&resp.body, b"blocked");
    }
}

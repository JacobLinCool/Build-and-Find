//! Middleware trait, chain, and the built-in middleware collection.

pub mod basic_auth;
pub mod cors;
pub mod csrf;
pub mod digest_auth;
pub mod gzip;
pub mod logger;
pub mod rate_limit;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::NanowebError;
use crate::http::types::{Request, Response};

/// Outcome of a middleware: either respond immediately (short-circuit)
/// or continue down the chain with a possibly-mutated request.
pub enum MiddlewareOutcome {
    /// Continue: pass the (possibly-mutated) request to the next middleware.
    Continue(Request),
    /// Stop here and return this response. Remaining middleware will
    /// still see it on the way out via `after`.
    Respond(Response),
}

/// The single middleware trait. Object-safe: stored as `Arc<dyn Middleware>`.
pub trait Middleware: Send + Sync {
    /// Short, stable name used by the logger middleware.
    fn name(&self) -> &'static str;

    /// Called before the router dispatches. Either passes the request
    /// along or short-circuits with a response.
    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<MiddlewareOutcome, NanowebError>> + Send + 'a>>;

    /// Called after the handler (or a short-circuited response) is produced.
    /// Default: pass-through.
    fn after<'a>(
        &'a self,
        _req: &'a Request,
        resp: Response,
    ) -> Pin<Box<dyn Future<Output = Result<Response, NanowebError>> + Send + 'a>> {
        Box::pin(async move { Ok(resp) })
    }
}

/// Ordered registration of middleware. Outer middleware (first
/// registered) see requests first and responses last.
#[derive(Default, Clone)]
pub struct Chain {
    entries: Vec<Arc<dyn Middleware>>,
}

impl Chain {
    pub fn new() -> Self {
        Chain {
            entries: Vec::new(),
        }
    }

    pub fn push<M: Middleware + 'static>(&mut self, m: M) -> &mut Self {
        self.entries.push(Arc::new(m));
        self
    }

    pub fn push_arc(&mut self, m: Arc<dyn Middleware>) -> &mut Self {
        self.entries.push(m);
        self
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.entries.iter().map(|m| m.name()).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Middleware>> {
        self.entries.iter()
    }

    /// Walk the chain. Middleware in registration order run `before`;
    /// a short-circuit skips the inner handler but `after` is still
    /// invoked in reverse order for every middleware already consulted.
    pub async fn run<F, Fut>(
        &self,
        req: Request,
        handler: F,
    ) -> Result<Response, NanowebError>
    where
        F: FnOnce(Request) -> Fut,
        Fut: Future<Output = Result<Response, NanowebError>>,
    {
        let mut current = req;
        let mut consulted = 0usize;
        let mut short_circuit: Option<Response> = None;
        for m in &self.entries {
            consulted += 1;
            let before_input = current.clone();
            match m.before(before_input).await? {
                MiddlewareOutcome::Continue(next) => current = next,
                MiddlewareOutcome::Respond(resp) => {
                    short_circuit = Some(resp);
                    break;
                }
            }
        }

        let mut resp = match short_circuit {
            Some(r) => r,
            None => handler(current.clone()).await?,
        };

        for m in self.entries.iter().take(consulted).rev() {
            resp = m.after(&current, resp).await?;
        }
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{Method, StatusCode};

    struct TagMiddleware(&'static str);
    impl Middleware for TagMiddleware {
        fn name(&self) -> &'static str {
            self.0
        }
        fn before<'a>(
            &'a self,
            mut req: Request,
        ) -> Pin<Box<dyn Future<Output = Result<MiddlewareOutcome, NanowebError>> + Send + 'a>>
        {
            Box::pin(async move {
                req.headers.append("x-trace", self.0);
                Ok(MiddlewareOutcome::Continue(req))
            })
        }
        fn after<'a>(
            &'a self,
            _req: &'a Request,
            mut resp: Response,
        ) -> Pin<Box<dyn Future<Output = Result<Response, NanowebError>> + Send + 'a>> {
            Box::pin(async move {
                resp.headers.append("x-out", self.0);
                Ok(resp)
            })
        }
    }

    #[test]
    fn registration_order_preserved() {
        let mut c = Chain::new();
        c.push(TagMiddleware("a"));
        c.push(TagMiddleware("b"));
        assert_eq!(c.names(), vec!["a", "b"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn chain_runs_before_and_after_in_expected_order() {
        let mut c = Chain::new();
        c.push(TagMiddleware("a"));
        c.push(TagMiddleware("b"));
        let req = Request::new(Method::Get, "/x");
        let resp = c
            .run(req, |_r| async { Ok(Response::status(StatusCode::OK)) })
            .await
            .unwrap();
        let outs: Vec<_> = resp
            .headers
            .iter()
            .filter(|(k, _)| *k == "x-out")
            .map(|(_, v)| v.to_string())
            .collect();
        // "after" runs in reverse of registration -> b then a
        assert_eq!(outs, vec!["b".to_string(), "a".to_string()]);
    }
}

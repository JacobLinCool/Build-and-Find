//! Middleware trait and chain composition.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::Result;
use crate::http::{Request, Response};

pub mod basic_auth;
pub mod compression;
pub mod cors;
pub mod csrf;
pub mod logging;
pub mod rate_limit;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A boxed handler whose future may borrow from the request.
pub type HandlerFn =
    dyn for<'r> Fn(&'r mut Request) -> BoxFuture<'r, Result<Response>> + Send + Sync;

/// `Next` represents the rest of the chain (later middleware + the handler).
pub struct Next<'a> {
    chain: &'a [Arc<dyn Middleware>],
    handler: &'a HandlerFn,
}

impl<'a> Next<'a> {
    pub fn new(chain: &'a [Arc<dyn Middleware>], handler: &'a HandlerFn) -> Self {
        Self { chain, handler }
    }

    pub fn run<'r>(self, req: &'r mut Request) -> BoxFuture<'r, Result<Response>>
    where
        'a: 'r,
    {
        if let Some((first, rest)) = self.chain.split_first() {
            let next = Next::new(rest, self.handler);
            first.handle(req, next)
        } else {
            (self.handler)(req)
        }
    }
}

/// Object-safe middleware trait.
///
/// Implementations may run logic before the handler, short-circuit by
/// returning a response without calling `next.run(req)`, or mutate the
/// response returned by `next.run(req)` before returning.
pub trait Middleware: Send + Sync {
    fn handle<'a>(
        &'a self,
        req: &'a mut Request,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<Response>>;
}

pub type MiddlewareList = Vec<Arc<dyn Middleware>>;

/// Run a middleware chain followed by the handler.
pub fn run_chain<'a>(
    chain: &'a [Arc<dyn Middleware>],
    handler: &'a HandlerFn,
    req: &'a mut Request,
) -> BoxFuture<'a, Result<Response>> {
    Next::new(chain, handler).run(req)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tag(&'static str);
    impl Middleware for Tag {
        fn handle<'a>(
            &'a self,
            req: &'a mut Request,
            next: Next<'a>,
        ) -> BoxFuture<'a, Result<Response>> {
            Box::pin(async move {
                req.extensions
                    .entry("trail".into())
                    .and_modify(|s| {
                        s.push(',');
                        s.push_str(self.0);
                    })
                    .or_insert_with(|| self.0.to_string());
                let mut resp = next.run(req).await?;
                resp.headers.append("X-Trail", self.0);
                Ok(resp)
            })
        }
    }

    fn make_handler() -> Box<HandlerFn> {
        Box::new(|_req: &mut Request| Box::pin(async move { Ok(Response::ok().text("x")) }))
    }

    #[tokio::test]
    async fn chain_runs_in_order() {
        let chain: MiddlewareList = vec![Arc::new(Tag("a")), Arc::new(Tag("b"))];
        let handler = make_handler();
        let mut req = Request::new();
        let resp = run_chain(&chain, handler.as_ref(), &mut req).await.unwrap();
        assert_eq!(req.extensions.get("trail").unwrap(), "a,b");
        let trails = resp.headers.get_all("X-Trail");
        assert_eq!(trails, vec!["b", "a"]);
    }
}

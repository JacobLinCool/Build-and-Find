pub mod basic_auth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limit;

use crate::http::{Request, Response};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub trait Middleware: Send + Sync {
    fn before(
        &self,
        _req: &mut Request,
    ) -> Pin<Box<dyn Future<Output = Option<Response>> + Send + 'static>> {
        Box::pin(async { None })
    }

    fn after(&self, _req: &Request, _resp: &mut Response) {}
}

pub async fn run_middleware_before(
    chain: &[Arc<dyn Middleware>],
    req: &mut Request,
) -> Option<Response> {
    for mw in chain {
        if let Some(resp) = mw.before(req).await {
            return Some(resp);
        }
    }
    None
}

pub fn run_middleware_after(chain: &[Arc<dyn Middleware>], req: &Request, resp: &mut Response) {
    for mw in chain.iter().rev() {
        mw.after(req, resp);
    }
}

pub async fn execute_with_middleware(
    chain: &[Arc<dyn Middleware>],
    mut req: Request,
    handler: impl Fn(Request) -> Pin<Box<dyn Future<Output = Response> + Send>>,
) -> Response {
    if let Some(resp) = run_middleware_before(chain, &mut req).await {
        return resp;
    }

    let mut resp = handler(req.clone()).await;

    run_middleware_after(chain, &req, &mut resp);

    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct TestMiddleware {
        called_before: Arc<AtomicBool>,
        called_after: Arc<AtomicBool>,
    }

    impl Middleware for TestMiddleware {
        fn before(
            &self,
            _req: &mut Request,
        ) -> Pin<Box<dyn Future<Output = Option<Response>> + Send + 'static>> {
            let called = self.called_before.clone();
            Box::pin(async move {
                called.store(true, Ordering::SeqCst);
                None
            })
        }

        fn after(&self, _req: &Request, _resp: &mut Response) {
            self.called_after.store(true, Ordering::SeqCst);
        }
    }

    struct ShortCircuitMiddleware;

    impl Middleware for ShortCircuitMiddleware {
        fn before(
            &self,
            _req: &mut Request,
        ) -> Pin<Box<dyn Future<Output = Option<Response>> + Send + 'static>> {
            Box::pin(async { Some(Response::new(403).with_body("Forbidden")) })
        }
    }

    #[tokio::test]
    async fn test_middleware_called() {
        let called_before = Arc::new(AtomicBool::new(false));
        let called_after = Arc::new(AtomicBool::new(false));
        let mw: Arc<dyn Middleware> = Arc::new(TestMiddleware {
            called_before: called_before.clone(),
            called_after: called_after.clone(),
        });
        let chain = vec![mw];

        let req = Request::new(crate::http::Method::Get, "/".into(), "HTTP/1.1".into());
        let resp = execute_with_middleware(&chain, req, |_req| {
            Box::pin(async { Response::ok().with_body("OK") })
        })
        .await;

        assert_eq!(resp.status, 200);
        assert!(called_before.load(Ordering::SeqCst));
        assert!(called_after.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_middleware_short_circuit() {
        let mw: Arc<dyn Middleware> = Arc::new(ShortCircuitMiddleware);
        let chain = vec![mw];

        let req = Request::new(crate::http::Method::Get, "/".into(), "HTTP/1.1".into());
        let resp = execute_with_middleware(&chain, req, |_req| {
            Box::pin(async { Response::ok().with_body("Should not reach") })
        })
        .await;

        assert_eq!(resp.status, 403);
    }
}

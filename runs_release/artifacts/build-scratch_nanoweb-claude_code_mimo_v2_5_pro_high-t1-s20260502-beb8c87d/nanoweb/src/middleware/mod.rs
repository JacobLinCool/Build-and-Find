pub mod basic_auth;
pub mod compression;
pub mod cors;
pub mod csrf;
pub mod logging;
pub mod rate_limit;

use crate::http::types::{Request, Response};

pub trait Middleware: Send + Sync {
    fn name(&self) -> &str;
    fn before(&self, _req: &mut Request) -> Option<Response> {
        None
    }
    fn after(&self, _req: &Request, _resp: &mut Response) {}
}

pub async fn run_middleware_chain(
    req: &mut Request,
    middleware: &[Box<dyn Middleware>],
    handler: impl FnOnce(
        Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>,
) -> Response {
    // Run before hooks
    for mw in middleware.iter() {
        if let Some(resp) = mw.before(req) {
            return resp;
        }
    }

    let mut resp = handler(req.clone()).await;

    // Run after hooks in reverse order
    for mw in middleware.iter().rev() {
        mw.after(req, &mut resp);
    }

    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestMiddleware {
        short_circuit: bool,
    }

    impl Middleware for TestMiddleware {
        fn name(&self) -> &str {
            "test"
        }
        fn before(&self, _req: &mut Request) -> Option<Response> {
            if self.short_circuit {
                Some(Response::text(403, "blocked"))
            } else {
                None
            }
        }
        fn after(&self, _req: &Request, resp: &mut Response) {
            resp.set_header("x-test", "applied");
        }
    }

    #[tokio::test]
    async fn test_middleware_short_circuit() {
        let mw: Vec<Box<dyn Middleware>> = vec![Box::new(TestMiddleware {
            short_circuit: true,
        })];
        let mut req = Request {
            method: crate::http::types::Method::Get,
            target: "/".to_string(),
            path: "/".to_string(),
            query: Default::default(),
            version: "HTTP/1.1".to_string(),
            headers: Default::default(),
            params: Default::default(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let resp = run_middleware_chain(&mut req, &mw, |req| {
            Box::pin(async move { Response::text(200, &req.path) })
        })
        .await;
        assert_eq!(resp.status, 403);
    }

    #[tokio::test]
    async fn test_middleware_after_hook() {
        let mw: Vec<Box<dyn Middleware>> = vec![Box::new(TestMiddleware {
            short_circuit: false,
        })];
        let mut req = Request {
            method: crate::http::types::Method::Get,
            target: "/".to_string(),
            path: "/".to_string(),
            query: Default::default(),
            version: "HTTP/1.1".to_string(),
            headers: Default::default(),
            params: Default::default(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let resp = run_middleware_chain(&mut req, &mw, |req| {
            Box::pin(async move { Response::text(200, &req.path) })
        })
        .await;
        assert_eq!(resp.status, 200);
        assert_eq!(resp.header("x-test").unwrap(), "applied");
    }
}

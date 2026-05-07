pub mod basic_auth;
pub mod compression;
pub mod cors;
pub mod csrf;
pub mod logging;
pub mod rate_limit;

use crate::http::{Request, Response};
use std::future::Future;
use std::pin::Pin;

/// Middleware trait - object safe version using manual future boxing.
pub trait MiddlewareDyn: Send + Sync {
    fn before<'a>(
        &'a self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Request, Response>> + Send + 'a>>;
    fn after<'a>(
        &'a self,
        request: &'a Request,
        response: Response,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>>;
}

/// Type alias for boxed middleware.
pub type BoxMiddleware = Box<dyn MiddlewareDyn>;

/// Run middleware chain: before -> handler -> after (in reverse order).
pub async fn run_middleware_before(
    middlewares: &[BoxMiddleware],
    mut req: Request,
) -> Result<Request, Response> {
    for mw in middlewares {
        req = mw.before(req).await?;
    }
    Ok(req)
}

/// Run middleware after chain in reverse order.
pub async fn run_middleware_after(
    middlewares: &[BoxMiddleware],
    req: &Request,
    mut resp: Response,
) -> Response {
    for mw in middlewares.iter().rev() {
        resp = mw.after(req, resp).await;
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HeaderMap, Method, StatusCode};

    struct TestMiddleware;

    impl MiddlewareDyn for TestMiddleware {
        fn before<'a>(
            &'a self,
            mut req: Request,
        ) -> Pin<Box<dyn Future<Output = Result<Request, Response>> + Send + 'a>> {
            Box::pin(async move {
                req.headers.insert("x-test", "applied");
                Ok(req)
            })
        }

        fn after<'a>(
            &'a self,
            _req: &'a Request,
            mut resp: Response,
        ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
            Box::pin(async move {
                resp.headers.insert("x-test-after", "true");
                resp
            })
        }
    }

    #[tokio::test]
    async fn test_middleware_before() {
        let mw: BoxMiddleware = Box::new(TestMiddleware);
        let req = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };

        let result = run_middleware_before(&[mw], req).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().header("x-test"), Some("applied"));
    }

    #[tokio::test]
    async fn test_middleware_short_circuit() {
        struct BlockMiddleware;
        impl MiddlewareDyn for BlockMiddleware {
            fn before<'a>(
                &'a self,
                _req: Request,
            ) -> Pin<Box<dyn Future<Output = Result<Request, Response>> + Send + 'a>> {
                Box::pin(async { Err(Response::text(StatusCode::FORBIDDEN, "blocked")) })
            }
            fn after<'a>(
                &'a self,
                _req: &'a Request,
                resp: Response,
            ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
                Box::pin(async move { resp })
            }
        }

        let mw: BoxMiddleware = Box::new(BlockMiddleware);
        let req = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };

        let result = run_middleware_before(&[mw], req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status, StatusCode::FORBIDDEN);
    }
}

pub mod basic_auth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limit;

use crate::http::{Request, Response};
use async_trait::async_trait;

#[async_trait]
pub trait Middleware: Send + Sync {
    async fn before(&self, _request: &mut Request) -> Option<Response> {
        None
    }

    async fn after(&self, _request: &Request, _response: &mut Response) {}
}

pub use basic_auth::BasicAuth;
pub use cors::Cors;
pub use csrf::Csrf;
pub use gzip::Gzip;
pub use logging::RequestLogger;
pub use rate_limit::RateLimit;

#[cfg(test)]
mod tests {
    use super::*;

    struct Noop;

    #[tokio::test]
    async fn default_trait_methods_do_nothing() {
        let mut req = Request::new(crate::http::Method::Get, "/");
        let mut res = Response::text(200, "ok");
        assert!(Noop.before(&mut req).await.is_none());
        Noop.after(&req, &mut res).await;
        assert_eq!(res.status, 200);
    }

    #[async_trait]
    impl Middleware for Noop {}
}

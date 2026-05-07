use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;

pub struct Csrf;
#[async_trait]
impl Middleware for Csrf {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        if matches!(
            req.method,
            crate::http::Method::Post
                | crate::http::Method::Put
                | crate::http::Method::Patch
                | crate::http::Method::Delete
        ) {
            let cookie = req.header("cookie").unwrap_or("");
            let token = req.header("x-csrf-token").unwrap_or("");
            if !cookie.contains("csrf_token=") || !cookie.contains(token) {
                return Some(Response::text(403, "csrf"));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn blocks_without_token() {
        let mut req = Request {
            method: crate::http::Method::Post,
            target: "/".into(),
            version: crate::http::Version::Http11,
            headers: Default::default(),
            body: vec![],
            path_params: Default::default(),
            query_params: Default::default(),
            remote_addr: None,
            request_id: "1".into(),
        };
        assert!(Csrf.before(&mut req).await.is_some());
    }
}

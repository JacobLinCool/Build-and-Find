use super::Middleware;
use crate::http::{Method, Request, Response};
use async_trait::async_trait;

#[derive(Default)]
pub struct Csrf;

#[async_trait]
impl Middleware for Csrf {
    async fn before(&self, request: &mut Request) -> Option<Response> {
        if matches!(
            request.method,
            Method::Get | Method::Head | Method::Options | Method::Other(_)
        ) {
            return None;
        }
        let cookie_token = request.header("cookie").and_then(extract_csrf_cookie);
        let header_token = request.header("x-csrf-token");
        if cookie_token.is_some() && cookie_token == header_token {
            None
        } else {
            Some(Response::text(403, "csrf token mismatch"))
        }
    }
}

fn extract_csrf_cookie(cookie: &str) -> Option<&str> {
    cookie.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == "csrf").then_some(value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_missing_token_for_post() {
        let mut req = Request::new(Method::Post, "/");
        assert_eq!(Csrf.before(&mut req).await.unwrap().status, 403);
    }
}

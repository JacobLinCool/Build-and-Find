use super::Middleware;
use crate::error::Result;
use crate::http::{Method, Request, Response, StatusCode};
use async_trait::async_trait;

pub struct Csrf;

#[async_trait]
impl Middleware for Csrf {
    async fn before(&self, req: &mut Request) -> Result<Option<Response>> {
        if matches!(
            req.method,
            Method::Post | Method::Put | Method::Patch | Method::Delete
        ) {
            let cookie_token = req.header("cookie").and_then(|c| {
                c.split(';')
                    .find_map(|p| p.trim().strip_prefix("csrf=").map(str::to_string))
            });
            let header_token = req.header("x-csrf-token").map(str::to_string);
            if cookie_token.is_none() || cookie_token != header_token {
                return Ok(Some(Response::new(StatusCode::FORBIDDEN, "csrf rejected")));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn rejects_missing_token_on_post() {
        let mut req = Request {
            method: Method::Post,
            target: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HashMap::new(),
            body: vec![],
            params: HashMap::new(),
            client_ip: None,
            request_id: "r".into(),
        };
        assert!(Csrf.before(&mut req).await.unwrap().is_some());
    }
}

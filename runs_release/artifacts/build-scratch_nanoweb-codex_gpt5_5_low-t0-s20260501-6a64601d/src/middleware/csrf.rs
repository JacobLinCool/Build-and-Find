use super::Middleware;
use crate::http::{Method, Request, Response, StatusCode};
use async_trait::async_trait;

pub struct Csrf;

#[async_trait]
impl Middleware for Csrf {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        if matches!(req.method, Method::Get | Method::Head | Method::Options) {
            return None;
        }
        let cookie = req.cookie("csrf_token");
        let header = req.header("x-csrf-token").map(ToOwned::to_owned);
        if cookie.is_some() && cookie == header {
            None
        } else {
            Some(Response::text(StatusCode::Forbidden, "csrf check failed"))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn construct() {
        let _ = super::Csrf;
    }
}

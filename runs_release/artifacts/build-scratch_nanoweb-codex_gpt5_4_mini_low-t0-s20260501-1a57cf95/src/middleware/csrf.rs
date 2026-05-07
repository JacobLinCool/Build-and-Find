use super::Middleware;
use crate::http::types::{Request, Response};

pub struct Csrf;
impl Middleware for Csrf {
    fn before(&self, req: &mut Request) -> Result<Option<Response>, crate::NanowebError> {
        if matches!(
            req.method,
            crate::http::types::Method::Get
                | crate::http::types::Method::Head
                | crate::http::types::Method::Options
        ) {
            return Ok(None);
        }
        let cookie = req.header("cookie").unwrap_or("");
        let token = req.header("x-csrf-token").unwrap_or("");
        if cookie.contains(&format!("csrf={}", token)) && !token.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Response::text(403, "csrf")))
        }
    }
}

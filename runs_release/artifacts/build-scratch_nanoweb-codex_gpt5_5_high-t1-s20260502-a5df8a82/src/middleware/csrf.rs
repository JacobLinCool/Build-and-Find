use async_trait::async_trait;
use rand::RngCore;

use crate::error::Result;
use crate::http::{Method, Request, Response, StatusCode};
use crate::middleware::{Middleware, cookie_value};

pub struct CsrfProtection;

#[async_trait]
impl Middleware for CsrfProtection {
    async fn before(&self, request: &mut Request) -> Result<Option<Response>> {
        let safe = matches!(request.method, Method::Get | Method::Head | Method::Options);
        if safe {
            return Ok(None);
        }
        let cookie = cookie_value(request.header("cookie"), "csrf_token");
        let header = request.header("x-csrf-token").map(str::to_string);
        if cookie.is_some() && cookie == header {
            Ok(None)
        } else {
            Ok(Some(Response::text(StatusCode::FORBIDDEN, "csrf rejected")))
        }
    }

    async fn after(&self, request: &Request, response: &mut Response) -> Result<()> {
        if cookie_value(request.header("cookie"), "csrf_token").is_none() {
            response.set_header(
                "Set-Cookie",
                format!("csrf_token={}; Path=/; SameSite=Lax", token()),
            );
        }
        Ok(())
    }
}

fn token() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn blocks_missing_token() {
        let mut req = Request::new(Method::Post, "/");
        assert!(CsrfProtection.before(&mut req).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn accepts_double_submit() {
        let mut req = Request::new(Method::Post, "/");
        req.set_header("Cookie", "csrf_token=a");
        req.set_header("X-CSRF-Token", "a");
        assert!(CsrfProtection.before(&mut req).await.unwrap().is_none());
    }
}

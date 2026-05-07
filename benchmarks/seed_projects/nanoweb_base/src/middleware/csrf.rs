//! Double-submit-cookie CSRF protection.
//!
//! Unsafe methods (POST/PUT/PATCH/DELETE) must supply the CSRF token in
//! both the cookie named `csrf_token` *and* the `X-CSRF-Token` header.
//! Safe methods are pass-through but the middleware ensures the cookie
//! is set on the way out.

use std::future::Future;
use std::pin::Pin;

use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response, StatusCode};
use crate::middleware::{Middleware, MiddlewareOutcome};

const COOKIE_NAME: &str = "csrf_token";
const HEADER_NAME: &str = "x-csrf-token";
const TOKEN_LEN: usize = 32;

pub struct CsrfMiddleware {
    pub cookie_name: String,
    pub header_name: String,
    pub secure: bool,
}

impl Default for CsrfMiddleware {
    fn default() -> Self {
        Self {
            cookie_name: COOKIE_NAME.into(),
            header_name: HEADER_NAME.into(),
            secure: false,
        }
    }
}

fn extract_cookie(raw: &str, name: &str) -> Option<String> {
    for part in raw.split(';') {
        let p = part.trim();
        if let Some(rest) = p.strip_prefix(&format!("{}=", name)) {
            return Some(rest.to_string());
        }
    }
    None
}

fn is_unsafe(method: Method) -> bool {
    matches!(
        method,
        Method::Post | Method::Put | Method::Patch | Method::Delete
    )
}

fn token_bytes() -> [u8; TOKEN_LEN] {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut out = [0u8; TOKEN_LEN];
    let base = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    for (i, b) in out.iter_mut().enumerate() {
        *b = ((base.wrapping_mul(6364136223846793005).wrapping_add(i as u64)) >> (i % 8)) as u8;
    }
    out
}

fn token_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

impl Middleware for CsrfMiddleware {
    fn name(&self) -> &'static str {
        "csrf"
    }

    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<MiddlewareOutcome, NanowebError>> + Send + 'a>>
    {
        Box::pin(async move {
            if !is_unsafe(req.method) {
                return Ok(MiddlewareOutcome::Continue(req));
            }
            let cookie_val = req
                .headers
                .get("cookie")
                .and_then(|c| extract_cookie(c, &self.cookie_name));
            let header_val = req.headers.get(&self.header_name).map(|s| s.to_string());
            match (cookie_val, header_val) {
                (Some(c), Some(h)) if constant_time_eq(c.as_bytes(), h.as_bytes()) => {
                    Ok(MiddlewareOutcome::Continue(req))
                }
                _ => {
                    let mut resp = Response::status(StatusCode::FORBIDDEN);
                    resp.headers.set("content-type", "text/plain");
                    resp.body = b"csrf: token mismatch".to_vec();
                    resp.headers
                        .set("content-length", resp.body.len().to_string());
                    Ok(MiddlewareOutcome::Respond(resp))
                }
            }
        })
    }

    fn after<'a>(
        &'a self,
        req: &'a Request,
        mut resp: Response,
    ) -> Pin<Box<dyn Future<Output = Result<Response, NanowebError>> + Send + 'a>> {
        Box::pin(async move {
            let cookie_present = req
                .headers
                .get("cookie")
                .and_then(|c| extract_cookie(c, &self.cookie_name))
                .is_some();
            if !cookie_present {
                let t = token_to_hex(&token_bytes());
                let flags = if self.secure {
                    "; Path=/; SameSite=Lax; Secure"
                } else {
                    "; Path=/; SameSite=Lax"
                };
                resp.headers.append(
                    "set-cookie",
                    format!("{}={}{}", self.cookie_name, t, flags),
                );
            }
            Ok(resp)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_when_tokens_missing() {
        let m = CsrfMiddleware::default();
        let req = Request::new(Method::Post, "/mutate");
        match m.before(req).await.unwrap() {
            MiddlewareOutcome::Respond(resp) => {
                assert_eq!(resp.status, StatusCode::FORBIDDEN);
            }
            _ => panic!("expected 403"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn accepts_when_tokens_match() {
        let m = CsrfMiddleware::default();
        let mut req = Request::new(Method::Post, "/mutate");
        req.headers.set("cookie", "csrf_token=abc");
        req.headers.set("x-csrf-token", "abc");
        assert!(matches!(
            m.before(req).await.unwrap(),
            MiddlewareOutcome::Continue(_)
        ));
    }

    #[test]
    fn constant_time_eq_matches_and_differs() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"ab", b"abc"));
    }
}

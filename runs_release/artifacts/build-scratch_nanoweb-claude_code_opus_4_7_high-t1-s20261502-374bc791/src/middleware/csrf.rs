//! Double-submit cookie CSRF protection.
//!
//! Safe methods (GET, HEAD, OPTIONS) pass through and have a CSRF token
//! cookie set if missing. Unsafe methods must include the token both as a
//! cookie and as a header (`X-CSRF-Token`) or form field; mismatch yields 403.

use crate::error::Result;
use crate::http::{Method, Request, Response, StatusCode};

use super::{BoxFuture, Middleware, Next};

pub struct CsrfMiddleware {
    pub cookie_name: String,
    pub header_name: String,
}

impl CsrfMiddleware {
    pub fn new() -> Self {
        Self {
            cookie_name: "csrf_token".into(),
            header_name: "X-CSRF-Token".into(),
        }
    }
}

impl Default for CsrfMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

fn cookie_value<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    for piece in cookie_header.split(';') {
        let p = piece.trim();
        if let Some(rest) = p.strip_prefix(&format!("{}=", name)) {
            return Some(rest);
        }
    }
    None
}

fn random_token() -> String {
    use base64::Engine;
    let id = uuid::Uuid::new_v4();
    let bytes = id.as_bytes();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn is_safe_method(m: Method) -> bool {
    matches!(m, Method::Get | Method::Head | Method::Options)
}

impl Middleware for CsrfMiddleware {
    fn handle<'a>(
        &'a self,
        req: &'a mut Request,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            let cookie_header = req.header("cookie").unwrap_or("").to_string();
            let cookie_token =
                cookie_value(&cookie_header, &self.cookie_name).map(|s| s.to_string());

            if !is_safe_method(req.method) {
                let header_token = req
                    .header(&self.header_name)
                    .map(|s| s.to_string())
                    .or_else(|| {
                        // Fall back to form field for x-www-form-urlencoded bodies.
                        if req
                            .header("content-type")
                            .map(str::to_ascii_lowercase)
                            .as_deref()
                            == Some("application/x-www-form-urlencoded")
                        {
                            let s = std::str::from_utf8(&req.body).ok()?;
                            crate::body::parse_query(s)
                                .into_iter()
                                .find(|(k, _)| k == "csrf_token")
                                .map(|(_, v)| v)
                        } else {
                            None
                        }
                    });
                let cookie = cookie_token.as_deref().ok_or_else(|| {
                    crate::error::NanowebError::Forbidden("csrf cookie missing".into())
                })?;
                let header = header_token.as_deref().ok_or_else(|| {
                    crate::error::NanowebError::Forbidden("csrf token missing".into())
                })?;
                if cookie != header {
                    return Ok(Response::new(StatusCode::FORBIDDEN).text("csrf token mismatch"));
                }
            }

            let mut resp = next.run(req).await?;
            if cookie_token.is_none() {
                let token = random_token();
                let cookie = format!("{}={}; Path=/; SameSite=Lax", self.cookie_name, token);
                resp.headers.append("set-cookie", cookie);
            }
            Ok(resp)
        })
    }
}

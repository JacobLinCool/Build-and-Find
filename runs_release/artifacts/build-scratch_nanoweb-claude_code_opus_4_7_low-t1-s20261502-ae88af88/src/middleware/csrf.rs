use super::Middleware;
use crate::http::{Method, Request, Response, StatusCode};
use async_trait::async_trait;
use rand::RngCore;

/// Double-submit cookie CSRF protection. The token is set as a cookie and must
/// also be sent in a header on unsafe methods.
pub struct Csrf {
    pub cookie_name: String,
    pub header_name: String,
}

impl Csrf {
    pub fn new() -> Self {
        Self {
            cookie_name: "csrf_token".into(),
            header_name: "X-CSRF-Token".into(),
        }
    }

    fn parse_cookie<'a>(&self, header: &'a str) -> Option<&'a str> {
        for part in header.split(';') {
            let p = part.trim();
            if let Some(rest) = p.strip_prefix(&format!("{}=", self.cookie_name)) {
                return Some(rest);
            }
        }
        None
    }

    fn generate() -> String {
        let mut buf = [0u8; 24];
        rand::thread_rng().fill_bytes(&mut buf);
        hex::encode(buf)
    }
}

impl Default for Csrf {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for Csrf {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        let unsafe_method = matches!(
            req.method,
            Method::Post | Method::Put | Method::Delete | Method::Patch
        );
        if !unsafe_method {
            return None;
        }
        let cookie = req
            .header("Cookie")
            .and_then(|c| self.parse_cookie(c).map(|s| s.to_string()));
        let header_token = req.header(&self.header_name).map(|s| s.to_string());
        match (cookie, header_token) {
            (Some(c), Some(h)) if c == h && !c.is_empty() => None,
            _ => {
                let mut r = Response::new(StatusCode::FORBIDDEN);
                r.body = b"csrf token mismatch".to_vec();
                Some(r)
            }
        }
    }

    async fn after(&self, req: &Request, res: &mut Response) {
        let has_cookie = req
            .header("Cookie")
            .map(|c| self.parse_cookie(c).is_some())
            .unwrap_or(false);
        if !has_cookie {
            let token = Self::generate();
            res.headers.append(
                "Set-Cookie",
                format!(
                    "{}={}; Path=/; SameSite=Strict; HttpOnly",
                    self.cookie_name, token
                ),
            );
        }
    }
}

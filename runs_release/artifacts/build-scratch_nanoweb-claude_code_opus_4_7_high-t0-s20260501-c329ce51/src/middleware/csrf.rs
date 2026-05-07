use super::Middleware;
use crate::http::{Method, Request, Response, StatusCode};
use async_trait::async_trait;
use rand::RngCore;

const COOKIE_NAME: &str = "csrf_token";
const HEADER_NAME: &str = "X-CSRF-Token";

/// Double-submit cookie CSRF protection. The token is stored in a cookie and
/// must be echoed back via the X-CSRF-Token header (or `_csrf` form field) on
/// state-changing requests.
pub struct Csrf {
    pub cookie_name: String,
    pub header_name: String,
}

impl Csrf {
    pub fn new() -> Self {
        Self {
            cookie_name: COOKIE_NAME.into(),
            header_name: HEADER_NAME.into(),
        }
    }
}

impl Default for Csrf {
    fn default() -> Self {
        Self::new()
    }
}

fn read_cookie(headers: &crate::http::HeaderMap, name: &str) -> Option<String> {
    let h = headers.get("Cookie")?;
    for piece in h.split(';') {
        let piece = piece.trim();
        if let Some((k, v)) = piece.split_once('=') {
            if k == name {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn random_token() -> String {
    let mut buf = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

fn is_safe_method(m: Method) -> bool {
    matches!(m, Method::Get | Method::Head | Method::Options)
}

#[async_trait]
impl Middleware for Csrf {
    fn name(&self) -> &'static str {
        "csrf"
    }

    async fn before(&self, req: &mut Request) -> Option<Response> {
        let cookie = read_cookie(&req.headers, &self.cookie_name);
        if is_safe_method(req.method) {
            // ensure cookie present; if not, mark to be set in after().
            if cookie.is_none() {
                let token = random_token();
                req.extensions.insert("csrf.set_cookie", &token);
            }
            return None;
        }
        let cookie_token = match cookie {
            Some(t) => t,
            None => {
                let mut r = Response::new(StatusCode::FORBIDDEN);
                r.body = b"missing csrf cookie".to_vec();
                return Some(r);
            }
        };
        let header_token = req.header(&self.header_name).map(|s| s.to_string());
        let body_token = if header_token.is_none()
            && req
                .header("Content-Type")
                .map(|c| c.starts_with("application/x-www-form-urlencoded"))
                .unwrap_or(false)
        {
            form_urlencoded::parse(&req.body)
                .find(|(k, _)| k == "_csrf")
                .map(|(_, v)| v.into_owned())
        } else {
            None
        };
        let provided = header_token.or(body_token);
        match provided {
            Some(t) if t == cookie_token => None,
            _ => {
                let mut r = Response::new(StatusCode::FORBIDDEN);
                r.body = b"csrf token mismatch".to_vec();
                Some(r)
            }
        }
    }

    async fn after(&self, req: &Request, resp: &mut Response) {
        if let Some(token) = req.extensions.get("csrf.set_cookie") {
            let cookie = format!(
                "{}={}; Path=/; HttpOnly; SameSite=Strict",
                self.cookie_name, token
            );
            resp.headers.append("Set-Cookie", cookie);
        }
    }
}

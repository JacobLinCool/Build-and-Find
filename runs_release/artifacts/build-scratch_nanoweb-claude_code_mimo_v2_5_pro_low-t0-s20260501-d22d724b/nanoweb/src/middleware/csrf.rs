use super::Middleware;
use crate::http::{Method, Request, Response};
use base64::Engine;
use rand::RngCore;
use std::future::Future;
use std::pin::Pin;

pub struct CsrfMiddleware {
    cookie_name: String,
    header_name: String,
    secure: bool,
}

impl CsrfMiddleware {
    pub fn new() -> Self {
        Self {
            cookie_name: "csrf_token".into(),
            header_name: "x-csrf-token".into(),
            secure: false,
        }
    }

    pub fn with_secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }

    fn generate_token() -> String {
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }
}

impl Middleware for CsrfMiddleware {
    fn before(
        &self,
        req: &mut Request,
    ) -> Pin<Box<dyn Future<Output = Option<Response>> + Send + 'static>> {
        let method = req.method;
        let header_name = self.header_name.clone();

        // Extract what we need before entering async
        let has_header = req.header(&header_name).is_some();
        let cookie_token = req
            .headers
            .iter()
            .find(|(k, _)| k.as_str() == "cookie")
            .and_then(|(_, v)| {
                v.split(';')
                    .find(|c| c.trim().starts_with("csrf_token="))
                    .map(|c| {
                        c.trim()
                            .strip_prefix("csrf_token=")
                            .unwrap_or("")
                            .to_string()
                    })
            });
        let header_token = req.header(&header_name).map(|s| s.to_string());

        Box::pin(async move {
            if method == Method::Get || method == Method::Head || method == Method::Options {
                return None;
            }

            if has_header {
                if let (Some(ct), Some(ht)) = (cookie_token, header_token) {
                    if ct == ht {
                        return None;
                    }
                }
            }

            Some(Response::new(403).with_body("CSRF token missing or invalid"))
        })
    }

    fn after(&self, _req: &Request, resp: &mut Response) {
        let token = CsrfMiddleware::generate_token();
        let cookie = if self.secure {
            format!(
                "{}={}; Path=/; HttpOnly; Secure; SameSite=Strict",
                self.cookie_name, token
            )
        } else {
            format!("{}={}; Path=/; SameSite=Strict", self.cookie_name, token)
        };
        resp.headers.push(("set-cookie".into(), cookie));
        resp.headers.push(("x-csrf-token".into(), token));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_csrf_allows_get() {
        let mw = CsrfMiddleware::new();
        let mut req = Request::new(Method::Get, "/".into(), "HTTP/1.1".into());
        let result = mw.before(&mut req).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_csrf_blocks_post_without_token() {
        let mw = CsrfMiddleware::new();
        let mut req = Request::new(Method::Post, "/".into(), "HTTP/1.1".into());
        let result = mw.before(&mut req).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().status, 403);
    }

    #[tokio::test]
    async fn test_csrf_sets_cookie() {
        let mw = CsrfMiddleware::new();
        let req = Request::new(Method::Get, "/".into(), "HTTP/1.1".into());
        let mut resp = Response::ok();
        mw.after(&req, &mut resp);
        assert!(resp.headers.iter().any(|(k, _)| k == "set-cookie"));
        assert!(resp.headers.iter().any(|(k, _)| k == "x-csrf-token"));
    }
}

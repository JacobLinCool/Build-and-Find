use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response};
use crate::middleware::{BoxFuture, Middleware, Next};
use rand::Rng;

const CSRF_COOKIE: &str = "csrf_token";
const CSRF_HEADER: &str = "x-csrf-token";
const CSRF_FIELD: &str = "_csrf";
const TOKEN_LEN: usize = 32;

pub struct CsrfMiddleware {
    cookie_name: String,
    header_name: String,
}

impl CsrfMiddleware {
    pub fn new() -> Self {
        CsrfMiddleware {
            cookie_name: CSRF_COOKIE.to_string(),
            header_name: CSRF_HEADER.to_string(),
        }
    }
}

impl Default for CsrfMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

fn generate_token() -> String {
    let bytes: Vec<u8> = (0..TOKEN_LEN)
        .map(|_| rand::thread_rng().gen::<u8>())
        .collect();
    hex::encode(bytes)
}

fn is_safe_method(method: &Method) -> bool {
    matches!(method, Method::Get | Method::Head | Method::Options)
}

fn parse_csrf_cookie(cookies: &str, name: &str) -> Option<String> {
    cookies.split(';').map(|c| c.trim()).find_map(|c| {
        if let Some(pos) = c.find('=') {
            let key = c[..pos].trim();
            let val = c[pos + 1..].trim();
            if key == name {
                Some(val.to_string())
            } else {
                None
            }
        } else {
            None
        }
    })
}

impl Middleware for CsrfMiddleware {
    fn call(&self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
        let cookie_name = self.cookie_name.clone();
        let header_name = self.header_name.clone();

        Box::pin(async move {
            let cookies = req.get_header("cookie").unwrap_or("").to_string();
            let cookie_token = parse_csrf_cookie(&cookies, &cookie_name);

            if !is_safe_method(&req.method) {
                // For state-changing requests, validate CSRF token
                let request_token =
                    req.get_header(&header_name)
                        .map(|s| s.to_string())
                        .or_else(|| {
                            // Could also look in form body but we keep it simple
                            None
                        });

                match (&cookie_token, &request_token) {
                    (Some(c), Some(r)) if c == r => {}
                    _ => return Err(NanowebError::Forbidden),
                }
            }

            let mut resp = next.run(req).await?;

            // Set CSRF token cookie if not present
            if cookie_token.is_none() {
                let token = generate_token();
                resp.add_header(
                    "Set-Cookie",
                    &format!("{}={}; Path=/; SameSite=Strict", cookie_name, token),
                );
            }

            Ok(resp)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::HttpVersion;
    use crate::middleware::{FnHandler, Next};
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_csrf_allows_get() {
        let handler = Arc::new(FnHandler::new(|_req: Request| async {
            Ok(Response::text("ok"))
        }));
        let csrf = Arc::new(CsrfMiddleware::new());
        let mw: Vec<Arc<dyn Middleware>> = vec![csrf];
        let next = Next::new(handler, mw);
        let req = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: HttpVersion::Http11,
            headers: vec![],
            body: vec![],
            params: HashMap::new(),
            remote_addr: "127.0.0.1".into(),
            request_id: "test".into(),
        };
        let resp = next.run(req).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn test_csrf_blocks_post_without_token() {
        let handler = Arc::new(FnHandler::new(|_req: Request| async {
            Ok(Response::text("ok"))
        }));
        let csrf = Arc::new(CsrfMiddleware::new());
        let mw: Vec<Arc<dyn Middleware>> = vec![csrf];
        let next = Next::new(handler, mw);
        let req = Request {
            method: Method::Post,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: HttpVersion::Http11,
            headers: vec![],
            body: vec![],
            params: HashMap::new(),
            remote_addr: "127.0.0.1".into(),
            request_id: "test".into(),
        };
        let resp = next.run(req).await;
        assert!(matches!(resp, Err(NanowebError::Forbidden)));
    }

    #[tokio::test]
    async fn test_csrf_allows_post_with_matching_token() {
        let handler = Arc::new(FnHandler::new(|_req: Request| async {
            Ok(Response::text("ok"))
        }));
        let csrf = Arc::new(CsrfMiddleware::new());
        let mw: Vec<Arc<dyn Middleware>> = vec![csrf];
        let next = Next::new(handler, mw);
        let token = "abc123";
        let req = Request {
            method: Method::Post,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: HttpVersion::Http11,
            headers: vec![
                ("cookie".to_string(), format!("csrf_token={}", token)),
                ("x-csrf-token".to_string(), token.to_string()),
            ],
            body: vec![],
            params: HashMap::new(),
            remote_addr: "127.0.0.1".into(),
            request_id: "test".into(),
        };
        let resp = next.run(req).await;
        assert!(resp.is_ok());
    }
}

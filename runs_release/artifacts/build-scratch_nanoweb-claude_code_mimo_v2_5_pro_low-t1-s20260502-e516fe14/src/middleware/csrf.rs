use super::MiddlewareDyn;
use crate::http::{Method, Request, Response, StatusCode};
use std::future::Future;
use std::pin::Pin;

/// CSRF protection using double-submit cookie pattern.
pub struct CsrfMiddleware {
    cookie_name: String,
    header_name: String,
}

impl CsrfMiddleware {
    pub fn new() -> Self {
        Self {
            cookie_name: "csrf_token".into(),
            header_name: "x-csrf-token".into(),
        }
    }

    fn is_safe_method(method: Method) -> bool {
        matches!(method, Method::Get | Method::Head | Method::Options)
    }
}

impl Default for CsrfMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl MiddlewareDyn for CsrfMiddleware {
    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Request, Response>> + Send + 'a>> {
        Box::pin(async move {
            // Safe methods don't need CSRF protection
            if Self::is_safe_method(req.method) {
                return Ok(req);
            }

            // Check that the CSRF cookie is present
            let cookie_token = req.header("cookie").and_then(|cookies| {
                cookies.split(';').find_map(|c| {
                    let c = c.trim();
                    if c.starts_with(&format!("{}=", self.cookie_name)) {
                        Some(c[self.cookie_name.len() + 1..].to_string())
                    } else {
                        None
                    }
                })
            });

            // Check that the CSRF header is present
            let header_token = req.header(&self.header_name).map(|s| s.to_string());

            match (cookie_token, header_token) {
                (Some(cookie), Some(header)) if cookie == header && !cookie.is_empty() => Ok(req),
                _ => Err(Response::text(
                    StatusCode::FORBIDDEN,
                    "CSRF token missing or mismatched",
                )),
            }
        })
    }

    fn after<'a>(
        &'a self,
        _req: &'a Request,
        resp: Response,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(async move { resp })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HeaderMap, Method};

    fn make_post_req(csrf_cookie: Option<&str>, csrf_header: Option<&str>) -> Request {
        let mut req = Request {
            method: Method::Post,
            target: "/".into(),
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };
        if let Some(token) = csrf_cookie {
            req.headers
                .insert("cookie", format!("csrf_token={}", token));
        }
        if let Some(token) = csrf_header {
            req.headers.insert("x-csrf-token", token);
        }
        req
    }

    #[tokio::test]
    async fn test_csrf_get_allowed() {
        let mw = CsrfMiddleware::new();
        let req = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };
        assert!(mw.before(req).await.is_ok());
    }

    #[tokio::test]
    async fn test_csrf_post_with_valid_token() {
        let mw = CsrfMiddleware::new();
        let req = make_post_req(Some("abc123"), Some("abc123"));
        assert!(mw.before(req).await.is_ok());
    }

    #[tokio::test]
    async fn test_csrf_post_without_token() {
        let mw = CsrfMiddleware::new();
        let req = make_post_req(None, None);
        assert!(mw.before(req).await.is_err());
    }

    #[tokio::test]
    async fn test_csrf_post_mismatched_token() {
        let mw = CsrfMiddleware::new();
        let req = make_post_req(Some("abc123"), Some("wrong"));
        assert!(mw.before(req).await.is_err());
    }
}

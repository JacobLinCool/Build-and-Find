use crate::error::NanowebError;
use crate::http::response::CookieOptions;
use crate::http::{Method, Request, Response};
use crate::middleware::{Middleware, Next};

const CSRF_COOKIE: &str = "csrf_token";
const CSRF_HEADER: &str = "x-csrf-token";
const CSRF_FIELD: &str = "_csrf";

pub struct CsrfMiddleware {
    pub cookie_name: String,
    pub header_name: String,
}

impl CsrfMiddleware {
    pub fn new() -> Self {
        Self {
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
    use uuid::Uuid;
    Uuid::new_v4().to_string().replace('-', "")
}

fn is_safe_method(method: &Method) -> bool {
    matches!(method, Method::Get | Method::Head | Method::Options)
}

#[async_trait::async_trait]
impl Middleware for CsrfMiddleware {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
        // Set CSRF token cookie if not present
        let cookie_token = req.cookie(&self.cookie_name);

        if !is_safe_method(&req.method) {
            // For unsafe methods, verify double-submit cookie
            let submitted = req
                .headers
                .get(&self.header_name)
                .map(|s| s.to_string())
                .or_else(|| {
                    // Also check form body
                    if let Some(ct) = req.content_type() {
                        if ct.contains("application/x-www-form-urlencoded") {
                            if let Ok(s) = std::str::from_utf8(&req.body) {
                                return extract_form_field(s, CSRF_FIELD);
                            }
                        }
                    }
                    None
                });

            match (cookie_token.as_deref(), submitted.as_deref()) {
                (Some(cookie), Some(submitted)) if cookie == submitted => {}
                _ => {
                    return Err(NanowebError::Forbidden("CSRF token mismatch".into()));
                }
            }
        }

        let mut response = next.run(req).await?;

        // Set/refresh CSRF cookie
        let token = cookie_token.unwrap_or_else(generate_token);
        let opts = CookieOptions::new().path("/").same_site("Strict");
        response.set_cookie(&self.cookie_name, &token, &opts);

        Ok(response)
    }
}

fn extract_form_field(body: &str, field: &str) -> Option<String> {
    for pair in body.split('&') {
        if let Some(eq) = pair.find('=') {
            if &pair[..eq] == field {
                return Some(pair[eq + 1..].to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Headers, Version};
    use crate::middleware::MiddlewareChain;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_get_passes_without_token() {
        let mw = Arc::new(CsrfMiddleware::new());
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok()) })
        });
        let chain = MiddlewareChain::new(vec![mw], handler);
        let req = Request::new(
            Method::Get,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        let resp = chain.run(req).await.unwrap();
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn test_post_blocked_without_token() {
        let mw = Arc::new(CsrfMiddleware::new());
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok()) })
        });
        let chain = MiddlewareChain::new(vec![mw], handler);
        let req = Request::new(
            Method::Post,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        let err = chain.run(req).await.unwrap_err();
        assert!(matches!(err, NanowebError::Forbidden(_)));
    }

    #[tokio::test]
    async fn test_post_allowed_with_matching_token() {
        let mw = Arc::new(CsrfMiddleware::new());
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok()) })
        });
        let chain = MiddlewareChain::new(vec![mw], handler);
        let mut req = Request::new(
            Method::Post,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        let token = "my-csrf-token";
        req.headers
            .insert("Cookie", format!("csrf_token={}", token));
        req.headers.insert("X-Csrf-Token", token);
        let resp = chain.run(req).await.unwrap();
        assert_eq!(resp.status, 200);
    }
}

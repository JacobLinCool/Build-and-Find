use super::{Middleware, Next};
use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response, Status};
use async_trait::async_trait;
use rand::Rng;

pub struct CsrfMiddleware {
    pub token_header: String,
    pub token_cookie: String,
}

impl Default for CsrfMiddleware {
    fn default() -> Self {
        CsrfMiddleware {
            token_header: "x-csrf-token".to_string(),
            token_cookie: "csrf_token".to_string(),
        }
    }
}

fn generate_token() -> String {
    let bytes: Vec<u8> = (0..32).map(|_| rand::thread_rng().gen::<u8>()).collect();
    hex::encode(bytes)
}

fn get_cookie_value<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(eq) = part.find('=') {
            if part[..eq].trim() == name {
                return Some(part[eq + 1..].trim());
            }
        }
    }
    None
}

fn safe_method(method: &Method) -> bool {
    matches!(method, Method::GET | Method::HEAD | Method::OPTIONS)
}

#[async_trait]
impl Middleware for CsrfMiddleware {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
        if !safe_method(&req.method) {
            // Validate CSRF token for state-changing methods
            let cookie_token = req
                .header("cookie")
                .and_then(|c| get_cookie_value(c, &self.token_cookie))
                .map(String::from);

            let header_token = req.header(&self.token_header).map(String::from);

            match (cookie_token, header_token) {
                (Some(ct), Some(ht)) if ct == ht => {
                    // Valid CSRF token
                }
                _ => {
                    return Ok(
                        Response::new(Status::Forbidden).with_text("CSRF token validation failed")
                    );
                }
            }
        }

        let mut resp = next.run(req).await?;

        // Set CSRF token cookie if not present
        let token = generate_token();
        let cookie = format!("{}={}; Path=/; SameSite=Strict", self.token_cookie, token);
        resp.headers.insert("set-cookie".to_string(), cookie);

        Ok(resp)
    }
}

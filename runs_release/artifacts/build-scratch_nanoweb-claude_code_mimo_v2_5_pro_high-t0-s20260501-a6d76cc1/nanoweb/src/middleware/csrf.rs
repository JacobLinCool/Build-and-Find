use super::Middleware;
use crate::http::{Method, Request, Response};
use async_trait::async_trait;
use rand::Rng;

const CSRF_TOKEN_LEN: usize = 32;

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

    fn generate_token(&self) -> String {
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..CSRF_TOKEN_LEN).map(|_| rng.gen()).collect();
        hex::encode(&bytes)
    }
}

// Simple hex encoding since we don't want an extra dep
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl Default for CsrfMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for CsrfMiddleware {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        let state_changing = matches!(
            req.method,
            Method::Post | Method::Put | Method::Delete | Method::Patch
        );
        if !state_changing {
            return None;
        }

        // Check for CSRF token in header or form body
        let header_token = req.header(&self.header_name).map(|s| s.to_string());

        // Also check _csrf in form body
        let form_token = if req
            .content_type()
            .map(|ct| ct.contains("application/x-www-form-urlencoded"))
            .unwrap_or(false)
        {
            let body_str = String::from_utf8_lossy(&req.body);
            serde_urlencoded::from_str::<std::collections::HashMap<String, String>>(&body_str)
                .ok()
                .and_then(|m| m.get("_csrf").cloned())
        } else {
            None
        };

        let token = header_token.or(form_token);

        // Get cookie token
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

        match (token, cookie_token) {
            (Some(t), Some(ct)) if t == ct => None, // valid
            _ => Some(Response::text(403, "CSRF token mismatch")),
        }
    }

    async fn after(&self, req: &Request, resp: &mut Response) {
        // Set CSRF cookie on safe requests if not present
        if matches!(req.method, Method::Get | Method::Head) {
            let has_cookie = req
                .header("cookie")
                .map(|c| c.contains(&self.cookie_name))
                .unwrap_or(false);

            if !has_cookie {
                let token = self.generate_token();
                resp.set_header(
                    "set-cookie",
                    format!(
                        "{}={}; Path=/; SameSite=Lax; HttpOnly",
                        self.cookie_name, token
                    ),
                );
            }
        }
    }
}

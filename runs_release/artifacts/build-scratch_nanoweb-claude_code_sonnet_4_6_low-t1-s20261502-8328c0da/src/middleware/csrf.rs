use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response, StatusCode};
use crate::middleware::{BoxFuture, Middleware, Next};
use rand::Rng;

pub struct CsrfMiddleware {
    pub cookie_name: String,
    pub header_name: String,
}

impl CsrfMiddleware {
    pub fn new() -> Self {
        CsrfMiddleware {
            cookie_name: "csrf_token".to_string(),
            header_name: "x-csrf-token".to_string(),
        }
    }

    fn generate_token() -> String {
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
        hex::encode(bytes)
    }

    fn extract_cookie(cookies: &str, name: &str) -> Option<String> {
        for part in cookies.split(';') {
            let part = part.trim();
            if let Some(eq_pos) = part.find('=') {
                let key = part[..eq_pos].trim();
                let val = part[eq_pos + 1..].trim();
                if key == name {
                    return Some(val.to_string());
                }
            }
        }
        None
    }
}

impl Default for CsrfMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for CsrfMiddleware {
    fn handle<'a>(&'a self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
        let cookie_name = self.cookie_name.clone();
        let header_name = self.header_name.clone();

        Box::pin(async move {
            // Safe methods don't need CSRF protection
            let needs_check = matches!(
                req.method,
                Method::Post | Method::Put | Method::Delete | Method::Patch
            );

            if needs_check {
                let cookie_token = req
                    .headers
                    .get("cookie")
                    .and_then(|c| CsrfMiddleware::extract_cookie(c, &cookie_name));
                let header_token = req.headers.get(&header_name).map(|s| s.to_string());

                match (cookie_token, header_token) {
                    (Some(cookie), Some(header)) if cookie == header => {}
                    _ => {
                        let mut resp = Response::new(StatusCode::FORBIDDEN);
                        resp.body = b"CSRF token invalid".to_vec();
                        return Ok(resp);
                    }
                }
            }

            let mut resp = next.run(req).await?;

            // Set CSRF cookie if not present
            let token = CsrfMiddleware::generate_token();
            let cookie_value = format!(
                "{}={}; Path=/; SameSite=Strict",
                cookie_name, token
            );
            resp.headers.set("set-cookie", cookie_value);

            Ok(resp)
        })
    }
}

use crate::http::{Method, Request, Response};
use crate::middleware::Middleware;
use base64::Engine;

pub struct CsrfMiddleware {
    secret: String,
}

impl CsrfMiddleware {
    pub fn new(secret: &str) -> Self {
        CsrfMiddleware {
            secret: secret.to_string(),
        }
    }

    fn generate_token(&self) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let nonce = uuid::Uuid::new_v4().to_string();
        let mut mac =
            HmacSha256::new_from_slice(self.secret.as_bytes()).expect("HMAC accepts any key size");
        mac.update(nonce.as_bytes());
        let result = mac.finalize();
        let sig = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            &result.into_bytes(),
        );
        format!("{}.{}", nonce, sig)
    }

    fn validate_token(&self, token: &str) -> bool {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let parts: Vec<&str> = token.splitn(2, '.').collect();
        if parts.len() != 2 {
            return false;
        }
        let nonce = parts[0];
        let sig_b64 = parts[1];

        let mut mac =
            HmacSha256::new_from_slice(self.secret.as_bytes()).expect("HMAC accepts any key size");
        mac.update(nonce.as_bytes());

        let sig_bytes = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(sig_b64) {
            Ok(b) => b,
            Err(_) => return false,
        };

        mac.verify_slice(&sig_bytes).is_ok()
    }

    fn extract_cookie_token<'a>(&self, req: &'a Request) -> Option<&'a str> {
        req.headers.get("cookie").and_then(|c| {
            c.split(';').find_map(|pair| {
                let pair = pair.trim();
                pair.strip_prefix("csrf_token=")
            })
        })
    }
}

impl Middleware for CsrfMiddleware {
    fn before(&self, req: &mut Request) -> Option<Response> {
        let is_safe = matches!(req.method, Method::Get | Method::Head | Method::Options);

        if is_safe {
            // Set cookie with new token if not present
            if self.extract_cookie_token(req).is_none() {
                let token = self.generate_token();
                req.extensions.insert("csrf_token".into(), token);
            }
            return None;
        }

        // For unsafe methods, validate the token
        let form_token = req.extensions.get("csrf_token").cloned().or_else(|| {
            // Check form body for csrf_token field
            if let Some(ct) = req.headers.content_type() {
                if ct.contains("application/x-www-form-urlencoded") {
                    let body = std::str::from_utf8(&req.body).ok()?;
                    body.split('&').find_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        let key = parts.next()?;
                        let val = parts.next()?;
                        if key == "csrf_token" {
                            Some(val.to_string())
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            } else {
                None
            }
        });

        let cookie_token = self.extract_cookie_token(req);

        match (form_token.as_deref(), cookie_token) {
            (Some(form), Some(cookie)) if self.validate_token(form) && form == cookie => {
                req.extensions.insert("csrf_token".into(), form.to_string());
                None
            }
            (Some(form), _) if self.validate_token(form) => {
                req.extensions.insert("csrf_token".into(), form.to_string());
                None
            }
            _ => {
                let mut res = Response::new();
                res.status = crate::http::StatusCode::new(403);
                res.set_text("CSRF token validation failed");
                Some(res)
            }
        }
    }

    fn after(&self, req: &Request, res: &mut Response) {
        if let Some(token) = req.extensions.get("csrf_token") {
            if !res.headers.contains("set-cookie") {
                res.headers.insert(
                    "set-cookie".into(),
                    format!("csrf_token={}; HttpOnly; SameSite=Strict", token),
                );
            }
        }
    }
}

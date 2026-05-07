use super::Middleware;
use crate::http::{Method, Request, Response, StatusCode};
use crate::NanowebError;
use async_trait::async_trait;
use rand::RngCore;

pub struct Csrf {
    pub cookie_name: String,
    pub header_name: String,
}

impl Default for Csrf {
    fn default() -> Self {
        Self {
            cookie_name: "csrf_token".into(),
            header_name: "X-CSRF-Token".into(),
        }
    }
}

fn parse_cookie<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    for part in header.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            if k == name {
                return Some(v);
            }
        }
    }
    None
}

fn random_token() -> String {
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[async_trait]
impl Middleware for Csrf {
    async fn before(&self, req: &mut Request) -> Result<Option<Response>, NanowebError> {
        let unsafe_method = matches!(
            req.method,
            Method::Post | Method::Put | Method::Delete | Method::Patch
        );
        if !unsafe_method {
            return Ok(None);
        }
        let cookie_header = req.header("cookie").unwrap_or("").to_string();
        let cookie_tok = parse_cookie(&cookie_header, &self.cookie_name);
        let header_tok = req.header(&self.header_name);
        match (cookie_tok, header_tok) {
            (Some(a), Some(b)) if !a.is_empty() && a == b => Ok(None),
            _ => {
                let mut r = Response::new(StatusCode::FORBIDDEN);
                r.body = b"csrf token mismatch".to_vec();
                Ok(Some(r))
            }
        }
    }

    async fn after(&self, req: &Request, mut resp: Response) -> Result<Response, NanowebError> {
        let cookie_header = req.header("cookie").unwrap_or("");
        if parse_cookie(cookie_header, &self.cookie_name).is_none() {
            let token = random_token();
            resp.headers.append(
                "Set-Cookie",
                format!("{}={}; Path=/; SameSite=Lax", self.cookie_name, token),
            );
        }
        Ok(resp)
    }
}

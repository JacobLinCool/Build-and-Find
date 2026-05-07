//! RFC 7617 "Basic" authentication.

use std::future::Future;
use std::pin::Pin;

use crate::error::NanowebError;
use crate::http::types::{Request, Response, StatusCode};
use crate::middleware::{Middleware, MiddlewareOutcome};

pub struct BasicAuthMiddleware {
    pub realm: String,
    /// list of (username, password) pairs. Kept in-memory for the seed.
    pub credentials: Vec<(String, String)>,
}

impl BasicAuthMiddleware {
    pub fn new(realm: impl Into<String>, credentials: Vec<(String, String)>) -> Self {
        Self {
            realm: realm.into(),
            credentials,
        }
    }

    fn check(&self, user: &str, pass: &str) -> bool {
        self.credentials
            .iter()
            .any(|(u, p)| u == user && p == pass)
    }

    fn challenge(&self) -> Response {
        let mut resp = Response::status(StatusCode::UNAUTHORIZED);
        resp.headers.set(
            "www-authenticate",
            format!("Basic realm=\"{}\", charset=\"UTF-8\"", self.realm),
        );
        resp
    }
}

fn b64_decode(input: &str) -> Option<Vec<u8>> {
    const TABLE: [i8; 128] = {
        let mut t = [-1i8; 128];
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i < alphabet.len() {
            t[alphabet[i] as usize] = i as i8;
            i += 1;
        }
        t
    };
    let input = input.trim_end_matches('=');
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for c in input.bytes() {
        if c >= 128 {
            return None;
        }
        let v = TABLE[c as usize];
        if v < 0 {
            return None;
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

fn parse_basic(header: &str) -> Option<(String, String)> {
    let rest = header.strip_prefix("Basic ").or_else(|| header.strip_prefix("basic "))?;
    let decoded = b64_decode(rest.trim())?;
    let s = std::str::from_utf8(&decoded).ok()?;
    let colon = s.find(':')?;
    Some((s[..colon].to_string(), s[colon + 1..].to_string()))
}

impl Middleware for BasicAuthMiddleware {
    fn name(&self) -> &'static str {
        "basic_auth"
    }

    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<MiddlewareOutcome, NanowebError>> + Send + 'a>>
    {
        Box::pin(async move {
            let raw = match req.headers.get("authorization") {
                Some(v) => v,
                None => return Ok(MiddlewareOutcome::Respond(self.challenge())),
            };
            match parse_basic(raw) {
                Some((u, p)) if self.check(&u, &p) => Ok(MiddlewareOutcome::Continue(req)),
                _ => Ok(MiddlewareOutcome::Respond(self.challenge())),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::Method;

    #[test]
    fn decodes_basic_header() {
        // "alice:secret" -> YWxpY2U6c2VjcmV0
        assert_eq!(
            parse_basic("Basic YWxpY2U6c2VjcmV0"),
            Some(("alice".to_string(), "secret".to_string()))
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_header_challenges() {
        let m = BasicAuthMiddleware::new("test", vec![("a".into(), "b".into())]);
        let req = Request::new(Method::Get, "/");
        match m.before(req).await.unwrap() {
            MiddlewareOutcome::Respond(r) => {
                assert_eq!(r.status, StatusCode::UNAUTHORIZED);
                assert!(r.headers.get("www-authenticate").is_some());
            }
            _ => panic!("expected challenge"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn accepts_valid_credentials() {
        let m = BasicAuthMiddleware::new("r", vec![("alice".into(), "secret".into())]);
        let mut req = Request::new(Method::Get, "/");
        req.headers.set("authorization", "Basic YWxpY2U6c2VjcmV0");
        assert!(matches!(
            m.before(req).await.unwrap(),
            MiddlewareOutcome::Continue(_)
        ));
    }
}

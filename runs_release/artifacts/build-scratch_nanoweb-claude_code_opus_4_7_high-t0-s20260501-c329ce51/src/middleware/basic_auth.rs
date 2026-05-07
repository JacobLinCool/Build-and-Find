use super::Middleware;
use crate::http::{Request, Response, StatusCode};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};

/// Basic-auth guard. Only authenticated requests are allowed to pass through.
pub struct BasicAuth {
    pub realm: String,
    pub users: Vec<(String, String)>,
}

impl BasicAuth {
    pub fn new(realm: impl Into<String>, users: Vec<(String, String)>) -> Self {
        Self {
            realm: realm.into(),
            users,
        }
    }

    fn check(&self, header: &str) -> bool {
        let b64 = match header
            .strip_prefix("Basic ")
            .or_else(|| header.strip_prefix("basic "))
        {
            Some(s) => s,
            None => return false,
        };
        let decoded = match STANDARD.decode(b64.trim()) {
            Ok(b) => b,
            Err(_) => return false,
        };
        let s = match std::str::from_utf8(&decoded) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let (u, p) = match s.split_once(':') {
            Some(t) => t,
            None => return false,
        };
        self.users
            .iter()
            .any(|(uu, pp)| uu == u && constant_eq(pp.as_bytes(), p.as_bytes()))
    }
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[async_trait]
impl Middleware for BasicAuth {
    fn name(&self) -> &'static str {
        "basic_auth"
    }

    async fn before(&self, req: &mut Request) -> Option<Response> {
        let h = req.header("Authorization").unwrap_or("");
        if self.check(h) {
            return None;
        }
        let mut r = Response::new(StatusCode::UNAUTHORIZED);
        r.headers.insert(
            "WWW-Authenticate",
            format!("Basic realm=\"{}\"", self.realm),
        );
        r.body = b"unauthorized".to_vec();
        Some(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, Version};

    #[tokio::test]
    async fn rejects_missing_auth() {
        let m = BasicAuth::new("test", vec![("u".into(), "p".into())]);
        let mut req = Request::new(Method::Get, "/".into(), Version::Http11);
        let resp = m.before(&mut req).await.unwrap();
        assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn allows_valid_auth() {
        let m = BasicAuth::new("test", vec![("u".into(), "p".into())]);
        let mut req = Request::new(Method::Get, "/".into(), Version::Http11);
        let creds = STANDARD.encode("u:p");
        req.headers
            .insert("Authorization", format!("Basic {}", creds));
        assert!(m.before(&mut req).await.is_none());
    }
}

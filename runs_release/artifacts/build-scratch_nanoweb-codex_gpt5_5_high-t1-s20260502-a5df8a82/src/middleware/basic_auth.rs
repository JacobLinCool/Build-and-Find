use async_trait::async_trait;
use base64::Engine;
use subtle::ConstantTimeEq;

use crate::error::Result;
use crate::http::{Request, Response, StatusCode};
use crate::middleware::Middleware;

pub struct BasicAuth {
    username: String,
    password: String,
    realm: String,
}

impl BasicAuth {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            realm: "nanoweb".into(),
        }
    }
}

#[async_trait]
impl Middleware for BasicAuth {
    async fn before(&self, request: &mut Request) -> Result<Option<Response>> {
        let expected = format!("{}:{}", self.username, self.password);
        let valid = request
            .header("authorization")
            .and_then(|v| v.strip_prefix("Basic "))
            .and_then(|v| base64::engine::general_purpose::STANDARD.decode(v).ok())
            .is_some_and(|decoded| decoded.ct_eq(expected.as_bytes()).into());
        if valid {
            Ok(None)
        } else {
            let mut response = Response::text(StatusCode::UNAUTHORIZED, "unauthorized");
            response.set_header(
                "WWW-Authenticate",
                format!("Basic realm=\"{}\"", self.realm),
            );
            Ok(Some(response))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;

    #[tokio::test]
    async fn accepts_valid_header() {
        let auth = BasicAuth::new("u", "p");
        let mut req = Request::new(Method::Get, "/");
        req.set_header(
            "Authorization",
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("u:p")
            ),
        );
        assert!(auth.before(&mut req).await.unwrap().is_none());
    }
}

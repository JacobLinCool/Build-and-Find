use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose};

pub struct BasicAuth {
    username: String,
    password: String,
}

impl BasicAuth {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }
}

#[async_trait]
impl Middleware for BasicAuth {
    async fn before(&self, request: &mut Request) -> Option<Response> {
        let expected = format!("{}:{}", self.username, self.password);
        let ok = request
            .header("authorization")
            .and_then(|h| h.strip_prefix("Basic "))
            .and_then(|b64| general_purpose::STANDARD.decode(b64).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .is_some_and(|actual| actual == expected);
        if ok {
            None
        } else {
            let mut response = Response::text(401, "authentication required");
            response.set_header("WWW-Authenticate", "Basic realm=\"nanoweb\"");
            Some(response)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn accepts_valid_credentials() {
        let auth = BasicAuth::new("u", "p");
        let mut req = Request::new(crate::http::Method::Get, "/");
        req.set_header(
            "Authorization",
            format!("Basic {}", general_purpose::STANDARD.encode("u:p")),
        );
        assert!(auth.before(&mut req).await.is_none());
    }
}

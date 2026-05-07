use async_trait::async_trait;
use base64::Engine;

use crate::error::Result;
use crate::http::{Request, Response, StatusCode};

use super::{Middleware, Next};

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

    fn unauthorized(&self) -> Response {
        Response::text(StatusCode::Unauthorized, "unauthorized").with_header(
            "www-authenticate",
            format!("Basic realm=\"{}\"", self.realm),
        )
    }
}

#[async_trait]
impl Middleware for BasicAuth {
    async fn handle(&self, request: Request, next: Next<'_>) -> Result<Response> {
        let Some(value) = request.header("authorization") else {
            return Ok(self.unauthorized());
        };
        let Some(token) = value.strip_prefix("Basic ") else {
            return Ok(self.unauthorized());
        };
        let decoded = match base64::engine::general_purpose::STANDARD.decode(token) {
            Ok(decoded) => decoded,
            Err(_) => return Ok(self.unauthorized()),
        };
        let credentials = String::from_utf8_lossy(&decoded);
        let mut parts = credentials.splitn(2, ':');
        let user = parts.next().unwrap_or_default();
        let pass = parts.next().unwrap_or_default();
        if user != self.username || pass != self.password {
            return Ok(self.unauthorized());
        }
        next.run(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HeaderMap, Method, Version};
    use crate::router::Handler;
    use std::sync::Arc;

    struct OkHandler;

    #[async_trait]
    impl Handler for OkHandler {
        async fn handle(&self, _request: Request) -> Result<Response> {
            Ok(Response::text(StatusCode::Ok, "ok"))
        }
    }

    #[tokio::test]
    async fn rejects_invalid_credentials() {
        let auth = BasicAuth::new("user", "pass");
        let request = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: Version::Http11,
            headers: HeaderMap::new(),
            body: vec![],
            params: Default::default(),
            remote_addr: None,
            request_id: "req".into(),
            session: None,
        };
        let response = crate::middleware::execute(&[Arc::new(auth)], Arc::new(OkHandler), request)
            .await
            .unwrap();
        assert_eq!(response.status, StatusCode::Unauthorized);
    }
}

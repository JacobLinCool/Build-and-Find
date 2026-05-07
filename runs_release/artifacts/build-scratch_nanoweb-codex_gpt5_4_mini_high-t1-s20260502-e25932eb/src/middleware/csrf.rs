use async_trait::async_trait;
use base64::Engine;
use rand::RngCore;

use crate::error::Result;
use crate::http::{Method, Request, Response, StatusCode};

use super::{Middleware, Next};

pub struct Csrf {
    cookie_name: String,
    header_name: String,
}

impl Default for Csrf {
    fn default() -> Self {
        Self::new("csrf_token", "x-csrf-token")
    }
}

impl Csrf {
    pub fn new(cookie_name: impl Into<String>, header_name: impl Into<String>) -> Self {
        Self {
            cookie_name: cookie_name.into(),
            header_name: header_name.into(),
        }
    }

    fn generate_token() -> String {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }
}

#[async_trait]
impl Middleware for Csrf {
    async fn handle(&self, request: Request, next: Next<'_>) -> Result<Response> {
        let cookies = request.cookies();
        let token = cookies
            .get(&self.cookie_name)
            .cloned()
            .unwrap_or_else(Self::generate_token);
        if !matches!(request.method, Method::Get | Method::Head | Method::Options) {
            let header_token = request
                .header(&self.header_name)
                .or_else(|| request.header("x-csrf-token"));
            if header_token != Some(token.as_str()) {
                return Ok(Response::text(StatusCode::Forbidden, "csrf token mismatch"));
            }
        }

        let mut response = next.run(request).await?;
        if cookies.get(&self.cookie_name).is_none() {
            response.headers.insert(
                "set-cookie",
                format!("{}={}; Path=/; SameSite=Lax", self.cookie_name, token),
            );
        }
        Ok(response)
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
    async fn rejects_missing_token() {
        let csrf = Csrf::default();
        let request = Request {
            method: Method::Post,
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
        let response = crate::middleware::execute(&[Arc::new(csrf)], Arc::new(OkHandler), request)
            .await
            .unwrap();
        assert_eq!(response.status, StatusCode::Forbidden);
    }
}

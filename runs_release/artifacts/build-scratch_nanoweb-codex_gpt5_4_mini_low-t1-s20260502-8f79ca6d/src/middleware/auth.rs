use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use base64::Engine;

pub struct BasicAuth {
    user: String,
    pass: String,
}
impl BasicAuth {
    pub fn new(user: impl Into<String>, pass: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            pass: pass.into(),
        }
    }
}
#[async_trait]
impl Middleware for BasicAuth {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        let header = req.header("authorization").unwrap_or("");
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", self.user, self.pass))
        );
        if header != expected {
            return Some(Response::text(401, "unauthorized"));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_missing_auth() {
        let mut req = Request {
            method: crate::http::Method::Get,
            target: "/".into(),
            version: crate::http::Version::Http11,
            headers: Default::default(),
            body: vec![],
            path_params: Default::default(),
            query_params: Default::default(),
            remote_addr: None,
            request_id: "1".into(),
        };
        assert!(BasicAuth::new("u", "p").before(&mut req).await.is_some());
    }
}

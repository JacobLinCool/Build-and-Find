use super::Middleware;
use crate::error::Result;
use crate::http::{Request, Response, StatusCode};
use async_trait::async_trait;
use base64::Engine;

pub struct BasicAuth {
    user: String,
    pass: String,
}

impl BasicAuth {
    pub fn new(user: String, pass: String) -> Self {
        Self { user, pass }
    }
}

#[async_trait]
impl Middleware for BasicAuth {
    async fn before(&self, req: &mut Request) -> Result<Option<Response>> {
        let ok = req
            .header("authorization")
            .and_then(|h| h.strip_prefix("Basic "))
            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
            .and_then(|v| String::from_utf8(v).ok())
            .is_some_and(|s| s == format!("{}:{}", self.user, self.pass));
        if ok {
            Ok(None)
        } else {
            let mut r = Response::new(StatusCode::UNAUTHORIZED, "unauthorized");
            r.headers
                .insert("www-authenticate".into(), "Basic realm=\"nanoweb\"".into());
            Ok(Some(r))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;
    use std::collections::HashMap;

    #[tokio::test]
    async fn rejects_missing_auth() {
        let mut req = Request {
            method: Method::Get,
            target: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HashMap::new(),
            body: vec![],
            params: HashMap::new(),
            client_ip: None,
            request_id: "r".into(),
        };
        assert!(
            BasicAuth::new("u".into(), "p".into())
                .before(&mut req)
                .await
                .unwrap()
                .is_some()
        );
    }
}

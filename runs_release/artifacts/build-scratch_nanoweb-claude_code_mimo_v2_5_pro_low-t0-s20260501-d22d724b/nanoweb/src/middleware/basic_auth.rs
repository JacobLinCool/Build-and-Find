use super::Middleware;
use crate::http::{Request, Response};
use base64::Engine;
use std::future::Future;
use std::pin::Pin;

pub struct BasicAuthMiddleware {
    username: String,
    password: String,
    realm: String,
}

impl BasicAuthMiddleware {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            realm: "Restricted".into(),
        }
    }

    pub fn with_realm(mut self, realm: impl Into<String>) -> Self {
        self.realm = realm.into();
        self
    }
}

impl Middleware for BasicAuthMiddleware {
    fn before(
        &self,
        req: &mut Request,
    ) -> Pin<Box<dyn Future<Output = Option<Response>> + Send + 'static>> {
        let username = self.username.clone();
        let password = self.password.clone();
        let realm = self.realm.clone();
        let auth_header = req.header("authorization").map(|s| s.to_string());

        Box::pin(async move {
            match auth_header {
                Some(auth) if auth.starts_with("Basic ") => {
                    let encoded = &auth[6..];
                    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                        if let Ok(credentials) = String::from_utf8(decoded) {
                            if let Some((user, pass)) = credentials.split_once(':') {
                                if user == username && pass == password {
                                    return None;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }

            Some(
                Response::new(401)
                    .with_header("www-authenticate", &format!("realm=\"{realm}\""))
                    .with_body("Unauthorized"),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_auth_blocks_unauthenticated() {
        let mw = BasicAuthMiddleware::new("admin", "secret");
        let mut req = Request::new(crate::http::Method::Get, "/".into(), "HTTP/1.1".into());
        let result = mw.before(&mut req).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().status, 401);
    }

    #[tokio::test]
    async fn test_basic_auth_allows_authenticated() {
        let mw = BasicAuthMiddleware::new("admin", "secret");
        let mut req = Request::new(crate::http::Method::Get, "/".into(), "HTTP/1.1".into());
        let credentials = base64::engine::general_purpose::STANDARD.encode("admin:secret");
        req.headers
            .insert("authorization".into(), format!("Basic {credentials}"));

        let result = mw.before(&mut req).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_basic_auth_rejects_wrong_password() {
        let mw = BasicAuthMiddleware::new("admin", "secret");
        let mut req = Request::new(crate::http::Method::Get, "/".into(), "HTTP/1.1".into());
        let credentials = base64::engine::general_purpose::STANDARD.encode("admin:wrong");
        req.headers
            .insert("authorization".into(), format!("Basic {credentials}"));

        let result = mw.before(&mut req).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().status, 401);
    }
}

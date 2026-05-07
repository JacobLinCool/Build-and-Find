use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use crate::middleware::{BoxFuture, Middleware, Next};
use base64::{engine::general_purpose, Engine};

pub struct BasicAuthMiddleware {
    realm: String,
    credentials: Vec<(String, String)>,
}

impl BasicAuthMiddleware {
    pub fn new(realm: &str, credentials: Vec<(String, String)>) -> Self {
        BasicAuthMiddleware {
            realm: realm.to_string(),
            credentials,
        }
    }

    pub fn single(realm: &str, username: &str, password: &str) -> Self {
        Self::new(realm, vec![(username.to_string(), password.to_string())])
    }
}

impl Middleware for BasicAuthMiddleware {
    fn call(&self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
        let realm = self.realm.clone();
        let credentials = self.credentials.clone();

        Box::pin(async move {
            let auth_header = req.get_header("authorization").map(|s| s.to_string());

            let authorized = match auth_header {
                None => false,
                Some(header) => {
                    if let Some(encoded) = header.strip_prefix("Basic ") {
                        match general_purpose::STANDARD.decode(encoded.trim()) {
                            Ok(decoded) => {
                                if let Ok(s) = String::from_utf8(decoded) {
                                    if let Some(colon) = s.find(':') {
                                        let user = &s[..colon];
                                        let pass = &s[colon + 1..];
                                        credentials.iter().any(|(u, p)| u == user && p == pass)
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            }
                            Err(_) => false,
                        }
                    } else {
                        false
                    }
                }
            };

            if authorized {
                next.run(req).await
            } else {
                let mut resp = Response::new(401);
                resp.set_header("WWW-Authenticate", &format!("Basic realm=\"{}\"", realm));
                resp.set_header("Content-Length", "0");
                Ok(resp)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{HttpVersion, Method};
    use crate::middleware::{FnHandler, Next};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn make_req_with_auth(auth: Option<&str>) -> Request {
        let mut headers = vec![];
        if let Some(a) = auth {
            headers.push(("authorization".to_string(), a.to_string()));
        }
        Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: HttpVersion::Http11,
            headers,
            body: vec![],
            params: HashMap::new(),
            remote_addr: "127.0.0.1".into(),
            request_id: "test".into(),
        }
    }

    #[tokio::test]
    async fn test_basic_auth_allows_correct_credentials() {
        let handler = Arc::new(FnHandler::new(|_req: Request| async {
            Ok(Response::text("ok"))
        }));
        let auth = Arc::new(BasicAuthMiddleware::single("Test", "admin", "secret"));
        let mw: Vec<Arc<dyn Middleware>> = vec![auth];
        let next = Next::new(handler, mw);
        let encoded = general_purpose::STANDARD.encode("admin:secret");
        let resp = next
            .run(make_req_with_auth(Some(&format!("Basic {}", encoded))))
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn test_basic_auth_blocks_missing_credentials() {
        let handler = Arc::new(FnHandler::new(|_req: Request| async {
            Ok(Response::text("ok"))
        }));
        let auth = Arc::new(BasicAuthMiddleware::single("Test", "admin", "secret"));
        let mw: Vec<Arc<dyn Middleware>> = vec![auth];
        let next = Next::new(handler, mw);
        let resp = next.run(make_req_with_auth(None)).await.unwrap();
        assert_eq!(resp.status, 401);
    }

    #[tokio::test]
    async fn test_basic_auth_blocks_wrong_credentials() {
        let handler = Arc::new(FnHandler::new(|_req: Request| async {
            Ok(Response::text("ok"))
        }));
        let auth = Arc::new(BasicAuthMiddleware::single("Test", "admin", "secret"));
        let mw: Vec<Arc<dyn Middleware>> = vec![auth];
        let next = Next::new(handler, mw);
        let encoded = general_purpose::STANDARD.encode("admin:wrong");
        let resp = next
            .run(make_req_with_auth(Some(&format!("Basic {}", encoded))))
            .await
            .unwrap();
        assert_eq!(resp.status, 401);
    }
}

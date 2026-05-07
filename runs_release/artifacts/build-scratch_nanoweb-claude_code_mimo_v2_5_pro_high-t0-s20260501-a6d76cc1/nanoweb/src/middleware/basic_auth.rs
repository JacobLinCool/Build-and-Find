use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;

pub struct BasicAuthMiddleware {
    pub username: String,
    pub password: String,
    pub realm: String,
}

impl BasicAuthMiddleware {
    pub fn new(username: String, password: String) -> Self {
        Self {
            username,
            password,
            realm: "Restricted".into(),
        }
    }
}

#[async_trait]
impl Middleware for BasicAuthMiddleware {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        let auth = req.header("authorization");
        match auth {
            Some(val) if val.starts_with("Basic ") => {
                let encoded = &val[6..];
                match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded) {
                    Ok(decoded) => {
                        let credentials = String::from_utf8_lossy(&decoded);
                        let parts: Vec<&str> = credentials.splitn(2, ':').collect();
                        if parts.len() == 2
                            && parts[0] == self.username
                            && parts[1] == self.password
                        {
                            return None;
                        }
                    }
                    Err(_) => {}
                }
                let mut resp = Response::text(401, "Unauthorized");
                resp.set_header(
                    "www-authenticate",
                    format!("Basic realm=\"{}\"", self.realm),
                );
                Some(resp)
            }
            _ => {
                let mut resp = Response::text(401, "Unauthorized");
                resp.set_header(
                    "www-authenticate",
                    format!("Basic realm=\"{}\"", self.realm),
                );
                Some(resp)
            }
        }
    }
}

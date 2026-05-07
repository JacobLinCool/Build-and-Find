use super::{Middleware, Next};
use crate::error::NanowebError;
use crate::http::types::{Request, Response, Status};
use async_trait::async_trait;
use base64::Engine;

pub struct BasicAuthMiddleware {
    pub username: String,
    pub password: String,
    pub realm: String,
}

impl BasicAuthMiddleware {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        BasicAuthMiddleware {
            username: username.into(),
            password: password.into(),
            realm: "nanoweb".to_string(),
        }
    }
}

#[async_trait]
impl Middleware for BasicAuthMiddleware {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
        let auth_header = req.header("authorization").map(String::from);

        let valid = if let Some(auth) = auth_header {
            if let Some(encoded) = auth.strip_prefix("Basic ") {
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(encoded.trim())
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok());

                if let Some(credentials) = decoded {
                    let parts: Vec<&str> = credentials.splitn(2, ':').collect();
                    parts.len() == 2 && parts[0] == self.username && parts[1] == self.password
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        if valid {
            next.run(req).await
        } else {
            Ok(Response::new(Status::Unauthorized)
                .with_text("Unauthorized")
                .set_header(
                    "www-authenticate",
                    format!("Basic realm=\"{}\"", self.realm),
                ))
        }
    }
}

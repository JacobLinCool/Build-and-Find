use crate::error::NanowebError;
use crate::http::types::{Request, Response, StatusCode};
use crate::middleware::{BoxFuture, Middleware, Next};
use base64::Engine;

pub struct BasicAuth {
    pub username: String,
    pub password: String,
    pub realm: String,
}

impl BasicAuth {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        BasicAuth {
            username: username.into(),
            password: password.into(),
            realm: "nanoweb".to_string(),
        }
    }
}

impl Middleware for BasicAuth {
    fn handle<'a>(&'a self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
        let username = self.username.clone();
        let password = self.password.clone();
        let realm = self.realm.clone();

        Box::pin(async move {
            let auth_header = req.headers.get("authorization").map(|s| s.to_string());

            let authenticated = if let Some(auth) = auth_header {
                if let Some(credentials) = auth.strip_prefix("Basic ") {
                    match base64::engine::general_purpose::STANDARD.decode(credentials.trim()) {
                        Ok(decoded) => {
                            if let Ok(decoded_str) = String::from_utf8(decoded) {
                                if let Some(colon_pos) = decoded_str.find(':') {
                                    let u = &decoded_str[..colon_pos];
                                    let p = &decoded_str[colon_pos + 1..];
                                    u == username && p == password
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
            } else {
                false
            };

            if !authenticated {
                let mut resp = Response::new(StatusCode::UNAUTHORIZED);
                resp.headers.set(
                    "www-authenticate",
                    format!("Basic realm=\"{}\"", realm),
                );
                resp.body = b"Unauthorized".to_vec();
                return Ok(resp);
            }

            next.run(req).await
        })
    }
}

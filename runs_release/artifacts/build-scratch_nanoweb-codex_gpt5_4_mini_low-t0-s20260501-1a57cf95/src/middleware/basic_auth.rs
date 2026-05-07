use super::Middleware;
use crate::http::types::{Request, Response};
use base64::Engine;

pub struct BasicAuth {
    pub user: Option<String>,
    pub pass: Option<String>,
}
impl Middleware for BasicAuth {
    fn before(&self, req: &mut Request) -> Result<Option<Response>, crate::NanowebError> {
        let (Some(user), Some(pass)) = (&self.user, &self.pass) else {
            return Ok(None);
        };
        let auth = req.header("authorization").unwrap_or("");
        let b64 = auth.strip_prefix("Basic ").unwrap_or("");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap_or_default();
        let creds = String::from_utf8_lossy(&decoded);
        if creds == format!("{}:{}", user, pass) {
            Ok(None)
        } else {
            Ok(Some(Response::text(401, "unauthorized")))
        }
    }
}

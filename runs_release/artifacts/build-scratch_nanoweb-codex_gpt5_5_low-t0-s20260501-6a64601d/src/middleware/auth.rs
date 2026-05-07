use super::Middleware;
use crate::http::{Request, Response, StatusCode};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose};

pub struct BasicAuth {
    username: String,
    password: String,
}

impl BasicAuth {
    pub fn new(username: String, password: String) -> Self {
        Self { username, password }
    }
}

#[async_trait]
impl Middleware for BasicAuth {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        let Some(auth) = req.header("authorization") else {
            return Some(challenge());
        };
        let Some(encoded) = auth.strip_prefix("Basic ") else {
            return Some(challenge());
        };
        let Ok(decoded) = general_purpose::STANDARD.decode(encoded) else {
            return Some(challenge());
        };
        let Ok(decoded) = String::from_utf8(decoded) else {
            return Some(challenge());
        };
        (decoded == format!("{}:{}", self.username, self.password))
            .then_some(())
            .map_or_else(|| Some(challenge()), |_| None)
    }
}

fn challenge() -> Response {
    let mut res = Response::text(StatusCode::Unauthorized, "unauthorized");
    res.header("WWW-Authenticate", "Basic realm=\"nanoweb\"");
    res
}

#[cfg(test)]
mod tests {
    #[test]
    fn construct() {
        let _ = super::BasicAuth::new("u".into(), "p".into());
    }
}

use crate::http::{Request, Response};
use crate::middleware::Middleware;

pub struct BasicAuthMiddleware {
    username: String,
    password: String,
}

impl BasicAuthMiddleware {
    pub fn new(username: &str, password: &str) -> Self {
        BasicAuthMiddleware {
            username: username.to_string(),
            password: password.to_string(),
        }
    }
}

impl Middleware for BasicAuthMiddleware {
    fn before(&self, req: &mut Request) -> Option<Response> {
        let auth_header = match req.headers.get("authorization") {
            Some(h) => h,
            None => return Some(self.unauthorized()),
        };
        if !auth_header.starts_with("Basic ") {
            return Some(self.unauthorized());
        }

        let encoded = &auth_header[6..];
        use base64::Engine;
        let decoded = match base64::engine::general_purpose::STANDARD.decode(encoded) {
            Ok(d) => d,
            Err(_) => return Some(self.unauthorized()),
        };
        let decoded_str = match String::from_utf8(decoded) {
            Ok(s) => s,
            Err(_) => return Some(self.unauthorized()),
        };

        let parts: Vec<&str> = decoded_str.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Some(self.unauthorized());
        }

        if parts[0] == self.username && parts[1] == self.password {
            req.extensions
                .insert("auth_user".into(), parts[0].to_string());
            None
        } else {
            Some(self.unauthorized())
        }
    }

    fn after(&self, _req: &Request, _res: &mut Response) {}
}

impl BasicAuthMiddleware {
    fn unauthorized(&self) -> Response {
        let mut res = Response::new();
        res.status = crate::http::StatusCode::new(401);
        res.headers
            .insert("www-authenticate".into(), "Basic realm=\"nanoweb\"".into());
        res.set_text("Unauthorized");
        res
    }
}

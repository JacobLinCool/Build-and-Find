use crate::http::request::Request;
use crate::http::response::Response;
use crate::middleware::Middleware;

pub struct BasicAuthMiddleware {
    username: String,
    password: String,
}

impl BasicAuthMiddleware {
    pub fn new(username: String, password: String) -> Self {
        BasicAuthMiddleware { username, password }
    }
}

impl Middleware for BasicAuthMiddleware {
    fn handle(&self, req: Request, next: Box<dyn FnOnce(Request) -> Response + Send>) -> Response {
        match req.header("authorization") {
            Some(auth) if auth.starts_with("Basic ") => {
                let encoded = &auth[6..];
                if let Ok(decoded) =
                    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
                {
                    if let Ok(s) = String::from_utf8(decoded) {
                        if let Some((user, pass)) = s.split_once(':') {
                            if user == self.username && pass == self.password {
                                return next(req);
                            }
                        }
                    }
                }
                unauthorized_response()
            }
            _ => unauthorized_response(),
        }
    }
}

fn unauthorized_response() -> Response {
    let mut resp = Response::new(401);
    resp.headers
        .insert("www-authenticate".into(), "Basic realm=\"nanoweb\"".into());
    resp.body = b"Unauthorized".to_vec();
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_auth_no_header() {
        let mw = BasicAuthMiddleware::new("user".into(), "pass".into());
        let req = Request {
            method: crate::http::parser::Method::Get,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            query: std::collections::HashMap::new(),
            remote_addr: None,
            request_id: "test".into(),
        };
        let resp = mw.handle(req, Box::new(|_| Response::text("ok")));
        assert_eq!(resp.status, 401);
    }

    #[test]
    fn test_basic_auth_valid() {
        let mw = BasicAuthMiddleware::new("admin".into(), "secret".into());
        let cred =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "admin:secret");
        let mut headers = std::collections::HashMap::new();
        headers.insert("authorization".into(), format!("Basic {cred}"));
        let req = Request {
            method: crate::http::parser::Method::Get,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers,
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            query: std::collections::HashMap::new(),
            remote_addr: None,
            request_id: "test".into(),
        };
        let resp = mw.handle(req, Box::new(|_| Response::text("ok")));
        assert_eq!(resp.status, 200);
    }
}

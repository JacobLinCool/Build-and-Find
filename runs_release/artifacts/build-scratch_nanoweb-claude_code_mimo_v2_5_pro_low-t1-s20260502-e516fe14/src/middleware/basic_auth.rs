use super::MiddlewareDyn;
use crate::http::{Request, Response, StatusCode};
use base64::Engine;
use std::future::Future;
use std::pin::Pin;

/// Basic authentication guard middleware.
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

    pub fn realm(mut self, realm: impl Into<String>) -> Self {
        self.realm = realm.into();
        self
    }
}

impl MiddlewareDyn for BasicAuthMiddleware {
    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Request, Response>> + Send + 'a>> {
        Box::pin(async move {
            let auth_header = req.header("authorization").unwrap_or("");

            if !auth_header.starts_with("Basic ") {
                return Err(unauthorized_response(&self.realm));
            }

            let encoded = &auth_header[6..];
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap_or_default();
            let credentials = String::from_utf8_lossy(&decoded);

            let mut parts = credentials.splitn(2, ':');
            let user = parts.next().unwrap_or("");
            let pass = parts.next().unwrap_or("");

            if user == self.username && pass == self.password {
                Ok(req)
            } else {
                Err(unauthorized_response(&self.realm))
            }
        })
    }

    fn after<'a>(
        &'a self,
        _req: &'a Request,
        resp: Response,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(async move { resp })
    }
}

fn unauthorized_response(realm: &str) -> Response {
    let mut resp = Response::text(StatusCode::UNAUTHORIZED, "Unauthorized");
    resp.headers
        .insert("www-authenticate", format!("Basic realm=\"{}\"", realm));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HeaderMap, Method};

    fn make_req(auth: Option<&str>) -> Request {
        let mut req = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };
        if let Some(val) = auth {
            req.headers.insert("authorization", val);
        }
        req
    }

    #[tokio::test]
    async fn test_basic_auth_valid() {
        let mw = BasicAuthMiddleware::new("admin", "secret");
        let encoded = base64::engine::general_purpose::STANDARD.encode("admin:secret");
        let req = make_req(Some(&format!("Basic {}", encoded)));
        assert!(mw.before(req).await.is_ok());
    }

    #[tokio::test]
    async fn test_basic_auth_invalid_password() {
        let mw = BasicAuthMiddleware::new("admin", "secret");
        let encoded = base64::engine::general_purpose::STANDARD.encode("admin:wrong");
        let req = make_req(Some(&format!("Basic {}", encoded)));
        assert!(mw.before(req).await.is_err());
    }

    #[tokio::test]
    async fn test_basic_auth_no_header() {
        let mw = BasicAuthMiddleware::new("admin", "secret");
        let req = make_req(None);
        let result = mw.before(req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status, StatusCode::UNAUTHORIZED);
    }
}

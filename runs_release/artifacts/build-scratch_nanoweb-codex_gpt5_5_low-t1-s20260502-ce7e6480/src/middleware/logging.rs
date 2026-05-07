use super::Middleware;
use crate::error::Result;
use crate::http::{Request, Response};
use async_trait::async_trait;
use tracing::info;

pub struct Logging;

#[async_trait]
impl Middleware for Logging {
    async fn before(&self, req: &mut Request) -> Result<Option<Response>> {
        info!(request_id=%req.request_id, method=%req.method.as_str(), path=%req.path(), "request_start");
        Ok(None)
    }

    async fn after(&self, req: &Request, res: Response) -> Result<Response> {
        info!(request_id=%req.request_id, status=res.status.0, "request_finish");
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;
    use std::collections::HashMap;

    #[tokio::test]
    async fn does_not_short_circuit() {
        let mut req = Request {
            method: Method::Get,
            target: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HashMap::new(),
            body: vec![],
            params: HashMap::new(),
            client_ip: None,
            request_id: "r".into(),
        };
        assert!(Logging.before(&mut req).await.unwrap().is_none());
    }
}

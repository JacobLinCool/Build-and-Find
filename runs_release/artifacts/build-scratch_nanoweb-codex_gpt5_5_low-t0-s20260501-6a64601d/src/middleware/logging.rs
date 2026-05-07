use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;

pub struct Logging;

#[async_trait]
impl Middleware for Logging {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        tracing::info!(request_id = %req.request_id, method = %req.method, path = %req.path, "request");
        None
    }

    async fn after(&self, req: &Request, res: Response) -> Response {
        tracing::info!(request_id = %req.request_id, status = res.status as u16, "response");
        res
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn construct() {
        let _ = super::Logging;
    }
}

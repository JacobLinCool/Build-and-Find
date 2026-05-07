use crate::http::types::{Request, Response};
use crate::middleware::Middleware;
use tracing::info;

pub struct RequestLogging;

impl Middleware for RequestLogging {
    fn before(&self, req: &mut Request) -> Option<Response> {
        tracing::info!(
            request_id = %req.request_id,
            method = %req.method,
            path = %req.path,
            "request received"
        );
        None
    }

    fn after(&self, req: &Request, mut resp: Response) -> Response {
        resp.headers.insert("x-request-id", req.request_id.clone());
        info!(
            request_id = %req.request_id,
            status = %resp.status.0,
            "request completed"
        );
        resp
    }

    fn name(&self) -> &str {
        "request-logging"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logging_middleware_name() {
        assert_eq!(RequestLogging.name(), "request-logging");
    }
}

use super::Middleware;
use crate::http::types::{Request, Response};
use tracing::info;

pub struct Logging;
impl Middleware for Logging {
    fn after(&self, req: &Request, resp: &mut Response) -> Result<(), crate::NanowebError> {
        info!(request_id=%req.request_id, method=%req.method.as_str(), target=%req.target, status=resp.status, "request complete");
        Ok(())
    }
}

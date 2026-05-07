use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;

pub struct RequestLogging;

#[async_trait]
impl Middleware for RequestLogging {
    async fn after(&self, req: &Request, resp: &mut Response) {
        println!(
            "request_id={} method={} path={} status={}",
            req.request_id,
            req.method.as_str(),
            req.target,
            resp.status
        );
    }
}

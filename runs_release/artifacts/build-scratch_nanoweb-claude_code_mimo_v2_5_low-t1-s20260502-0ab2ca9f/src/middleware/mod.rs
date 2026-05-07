pub mod basic_auth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limit;

use crate::http::{Request, Response};

pub trait Middleware: Send + Sync {
    fn before(&self, _req: &mut Request) -> Option<Response> {
        None
    }

    fn after(&self, _req: &Request, _res: &mut Response) {}
}

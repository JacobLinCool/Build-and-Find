use super::Middleware;
use crate::http::types::{Request, Response};

pub struct Cors {
    pub allowed_origins: Vec<String>,
}
impl Middleware for Cors {
    fn after(&self, req: &Request, resp: &mut Response) -> Result<(), crate::NanowebError> {
        if let Some(origin) = req.header("origin") {
            if self.allowed_origins.iter().any(|o| o == "*" || o == origin) {
                resp.headers
                    .insert("access-control-allow-origin".into(), origin.into());
            }
        }
        Ok(())
    }
}

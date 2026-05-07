use crate::http::types::{Request, Response};

pub trait Middleware: Send + Sync {
    fn before(&self, _req: &mut Request) -> Result<Option<Response>, crate::NanowebError> {
        Ok(None)
    }
    fn after(&self, _req: &Request, _resp: &mut Response) -> Result<(), crate::NanowebError> {
        Ok(())
    }
}

pub mod basic_auth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limit;

#[cfg(test)]
mod tests {
    use super::*;
    struct M;
    impl Middleware for M {}
    #[test]
    fn default_ok() {
        let mut req = Request {
            method: crate::http::types::Method::Get,
            target: "/".into(),
            version: "HTTP/1.1".into(),
            headers: Default::default(),
            body: vec![],
            params: Default::default(),
            peer_addr: None,
            request_id: String::new(),
        };
        let _ = M.before(&mut req).unwrap();
    }
}

use super::Middleware;
use crate::http::types::{Request, Response};
use flate2::{write::GzEncoder, Compression};
use std::io::Write;

pub struct Gzip;
impl Middleware for Gzip {
    fn after(&self, req: &Request, resp: &mut Response) -> Result<(), crate::NanowebError> {
        if req
            .header("accept-encoding")
            .map_or(false, |v| v.contains("gzip"))
            && !resp.body.is_empty()
        {
            let mut enc = GzEncoder::new(Vec::new(), Compression::default());
            enc.write_all(&resp.body)?;
            resp.body = enc.finish()?;
            resp.headers
                .insert("content-encoding".into(), "gzip".into());
        }
        Ok(())
    }
}

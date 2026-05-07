use super::Middleware;
use crate::error::Result;
use crate::http::{Request, Response};
use async_trait::async_trait;
use flate2::{Compression, write::GzEncoder};
use std::io::Write;

pub struct Gzip;

#[async_trait]
impl Middleware for Gzip {
    async fn after(&self, req: &Request, mut res: Response) -> Result<Response> {
        if req.header("accept-encoding").unwrap_or("").contains("gzip")
            && !res.body.is_empty()
            && !res.headers.contains_key("content-encoding")
        {
            let mut enc = GzEncoder::new(Vec::new(), Compression::default());
            enc.write_all(&res.body)?;
            res.body = enc.finish()?;
            res.headers.insert("content-encoding".into(), "gzip".into());
            res.headers.remove("content-length");
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, StatusCode};
    use std::collections::HashMap;

    #[tokio::test]
    async fn adds_gzip_header() {
        let mut headers = HashMap::new();
        headers.insert("accept-encoding".into(), "gzip".into());
        let req = Request {
            method: Method::Get,
            target: "/".into(),
            version: "HTTP/1.1".into(),
            headers,
            body: vec![],
            params: HashMap::new(),
            client_ip: None,
            request_id: "r".into(),
        };
        let res = Gzip
            .after(&req, Response::new(StatusCode::OK, "hello"))
            .await
            .unwrap();
        assert_eq!(res.headers["content-encoding"], "gzip");
    }
}

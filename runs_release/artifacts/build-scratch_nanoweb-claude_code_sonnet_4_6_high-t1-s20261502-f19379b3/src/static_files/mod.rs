use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct StaticFileHandler {
    root: PathBuf,
    route_prefix: String,
}

impl StaticFileHandler {
    pub fn new(root: impl Into<PathBuf>, route_prefix: impl Into<String>) -> Self {
        StaticFileHandler {
            root: root.into(),
            route_prefix: route_prefix.into(),
        }
    }

    pub async fn serve(&self, req: &Request) -> Result<Response, NanowebError> {
        // Only serve GET and HEAD
        if req.method != Method::Get && req.method != Method::Head {
            return Err(NanowebError::MethodNotAllowed(vec![
                "GET".into(),
                "HEAD".into(),
            ]));
        }

        let rel_path = req
            .path
            .strip_prefix(&self.route_prefix)
            .unwrap_or(&req.path)
            .trim_start_matches('/');

        // Prevent directory traversal
        let resolved = self.root.join(rel_path);
        let canonical = resolved
            .canonicalize()
            .map_err(|_| NanowebError::NotFound)?;

        let root_canonical = self
            .root
            .canonicalize()
            .map_err(|e| NanowebError::Internal(e.to_string()))?;

        if !canonical.starts_with(&root_canonical) {
            return Err(NanowebError::Forbidden);
        }

        if canonical.is_dir() {
            // Try index.html
            let index = canonical.join("index.html");
            if index.exists() {
                return self.serve_file(&req, &index).await;
            }
            return Err(NanowebError::NotFound);
        }

        self.serve_file(&req, &canonical).await
    }

    async fn serve_file(&self, req: &Request, path: &Path) -> Result<Response, NanowebError> {
        let meta = tokio::fs::metadata(path)
            .await
            .map_err(|_| NanowebError::NotFound)?;

        let mtime = meta
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH)
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let size = meta.len();
        let etag = format!("W/\"{}-{}\"", mtime, size);

        // Check If-None-Match
        if let Some(inm) = req.get_header("if-none-match") {
            if inm == etag {
                let mut resp = Response::new(304);
                resp.set_header("ETag", &etag);
                return Ok(resp);
            }
        }

        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();

        let mut resp = if req.method == Method::Head {
            let mut r = Response::new(200);
            r.set_header("Content-Length", &size.to_string());
            r
        } else {
            let body = tokio::fs::read(path).await.map_err(NanowebError::Io)?;
            Response::with_body(200, body)
        };

        resp.set_header("Content-Type", &mime);
        resp.set_header("ETag", &etag);
        resp.set_header("Cache-Control", "public, max-age=3600");

        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::HttpVersion;
    use std::collections::HashMap;
    use tempfile::TempDir;

    async fn make_req(method: Method, path: &str) -> Request {
        Request {
            method,
            target: path.to_string(),
            path: path.to_string(),
            query: None,
            version: HttpVersion::Http11,
            headers: vec![],
            body: vec![],
            params: HashMap::new(),
            remote_addr: "127.0.0.1".into(),
            request_id: "test".into(),
        }
    }

    #[tokio::test]
    async fn test_serve_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("hello.txt");
        tokio::fs::write(&file_path, b"hello world" as &[u8])
            .await
            .unwrap();

        let handler = StaticFileHandler::new(dir.path().to_path_buf(), "/static");
        let req = make_req(Method::Get, "/static/hello.txt").await;
        let resp = handler.serve(&req).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello world");
    }

    #[tokio::test]
    async fn test_directory_traversal_blocked() {
        let dir = TempDir::new().unwrap();
        let handler = StaticFileHandler::new(dir.path().to_path_buf(), "/static");
        let req = make_req(Method::Get, "/static/../etc/passwd").await;
        let resp = handler.serve(&req).await;
        assert!(matches!(
            resp,
            Err(NanowebError::NotFound) | Err(NanowebError::Forbidden)
        ));
    }

    #[tokio::test]
    async fn test_etag_304() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        tokio::fs::write(&file_path, b"content" as &[u8])
            .await
            .unwrap();

        let handler = StaticFileHandler::new(dir.path().to_path_buf(), "/static");
        let req1 = make_req(Method::Get, "/static/test.txt").await;
        let resp1 = handler.serve(&req1).await.unwrap();
        let etag = resp1.get_header("etag").unwrap().to_string();

        let mut req2 = make_req(Method::Get, "/static/test.txt").await;
        req2.headers.push(("if-none-match".to_string(), etag));
        let resp2 = handler.serve(&req2).await.unwrap();
        assert_eq!(resp2.status, 304);
    }
}

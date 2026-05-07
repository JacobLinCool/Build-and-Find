use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use std::path::PathBuf;

pub struct StaticFileHandler {
    root: PathBuf,
}

impl StaticFileHandler {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn handle(&self, req: &Request) -> Result<Option<Response>, NanowebError> {
        let path = self.resolve_path(&req.path)?;
        if path.is_none() {
            return Ok(None);
        }
        let path = path.unwrap();

        let metadata = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => return Ok(None),
        };

        if !metadata.is_file() {
            return Ok(None);
        }

        let etag = generate_etag(&metadata);
        if let Some(if_none_match) = req.header("if-none-match") {
            if if_none_match == etag {
                let mut resp = Response::new(304);
                resp.set_header("etag", &etag);
                return Ok(Some(resp));
            }
        }

        let content_type = mime_guess::from_path(&path)
            .first_or_octet_stream()
            .to_string();

        let contents = std::fs::read(&path)?;

        let mut resp = Response::ok();
        resp.set_header("content-type", &content_type);
        resp.set_header("etag", &etag);
        resp.body = contents;

        Ok(Some(resp))
    }

    fn resolve_path(&self, request_path: &str) -> Result<Option<PathBuf>, NanowebError> {
        // Normalize path: strip leading slash, decode percent-encoding
        let path = request_path.trim_start_matches('/');

        // Reject directory traversal
        if path.contains("..") || path.contains('\\') {
            return Err(NanowebError::Other(
                "Directory traversal rejected".to_string(),
            ));
        }

        let full_path = self.root.join(path);

        // Canonicalize and verify it's under root
        let canonical = match full_path.canonicalize() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        let canonical_root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());

        if !canonical.starts_with(&canonical_root) {
            return Err(NanowebError::Other(
                "Directory traversal rejected".to_string(),
            ));
        }

        Ok(Some(canonical))
    }
}

fn generate_etag(metadata: &std::fs::Metadata) -> String {
    use std::time::UNIX_EPOCH;
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let size = metadata.len();
    format!("W/\"{mtime:x}-{size:x}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_serve_static_file() {
        let dir = std::env::temp_dir().join("nanoweb_static_test");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("test.txt"), "hello world").unwrap();

        let handler = StaticFileHandler::new(&dir);
        let req = Request {
            method: crate::http::types::Method::Get,
            target: "/test.txt".to_string(),
            path: "/test.txt".to_string(),
            query: Default::default(),
            version: "HTTP/1.1".to_string(),
            headers: Default::default(),
            params: Default::default(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let resp = handler.handle(&req).unwrap().unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello world");
        assert!(resp.header("etag").is_some());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_directory_traversal_rejected() {
        let dir = std::env::temp_dir().join("nanoweb_static_test2");
        let _ = fs::create_dir_all(&dir);

        let handler = StaticFileHandler::new(&dir);
        let req = Request {
            method: crate::http::types::Method::Get,
            target: "/../../../etc/passwd".to_string(),
            path: "/../../../etc/passwd".to_string(),
            query: Default::default(),
            version: "HTTP/1.1".to_string(),
            headers: Default::default(),
            params: Default::default(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let result = handler.handle(&req);
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_304_not_modified() {
        let dir = std::env::temp_dir().join("nanoweb_static_test3");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("page.html"), "<h1>Hi</h1>").unwrap();

        let handler = StaticFileHandler::new(&dir);

        // First request to get ETag
        let req1 = Request {
            method: crate::http::types::Method::Get,
            target: "/page.html".to_string(),
            path: "/page.html".to_string(),
            query: Default::default(),
            version: "HTTP/1.1".to_string(),
            headers: Default::default(),
            params: Default::default(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let resp1 = handler.handle(&req1).unwrap().unwrap();
        let etag = resp1.header("etag").unwrap().to_string();

        // Second request with If-None-Match
        let mut headers = std::collections::HashMap::new();
        headers.insert("if-none-match".to_string(), etag);
        let req2 = Request {
            method: crate::http::types::Method::Get,
            target: "/page.html".to_string(),
            path: "/page.html".to_string(),
            query: Default::default(),
            version: "HTTP/1.1".to_string(),
            headers,
            params: Default::default(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let resp2 = handler.handle(&req2).unwrap().unwrap();
        assert_eq!(resp2.status, 304);

        let _ = fs::remove_dir_all(&dir);
    }
}

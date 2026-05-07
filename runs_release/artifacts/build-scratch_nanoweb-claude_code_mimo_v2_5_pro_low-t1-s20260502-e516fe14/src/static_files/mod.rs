use crate::http::{Request, Response, StatusCode};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Static file server with ETag and If-None-Match support.
pub struct StaticFileServer {
    root: PathBuf,
}

impl StaticFileServer {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let canonical_root = root.canonicalize().unwrap_or(root);
        Self {
            root: canonical_root,
        }
    }

    /// Serve a static file for the given request path.
    pub async fn serve(&self, req: &Request) -> Option<Response> {
        let path = req.path.strip_prefix('/').unwrap_or(&req.path);

        // Reject directory traversal
        if path.contains("..") || path.contains('\\') {
            return Some(Response::text(
                StatusCode::FORBIDDEN,
                "Directory traversal not allowed",
            ));
        }

        let file_path = self.root.join(path);

        // Resolve the path and ensure it's under root
        let canonical = match file_path.canonicalize() {
            Ok(p) => p,
            Err(_) => return None,
        };

        if !canonical.starts_with(&self.root) {
            return Some(Response::text(StatusCode::FORBIDDEN, "Access denied"));
        }

        // If it's a directory, try index.html
        let file_path = if canonical.is_dir() {
            canonical.join("index.html")
        } else {
            canonical
        };

        let metadata = match fs::metadata(&file_path).await {
            Ok(m) => m,
            Err(_) => return None,
        };

        if !metadata.is_file() {
            return None;
        }

        // Generate weak ETag from mtime + size
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let size = metadata.len();
        let etag = format!("W/\"{:x}-{:x}\"", mtime, size);

        // Check If-None-Match
        if let Some(if_none_match) = req.header("if-none-match") {
            if if_none_match == etag || if_none_match.contains(&etag) {
                let mut resp = Response::new(StatusCode::NOT_MODIFIED);
                resp.headers.insert("etag", &etag);
                return Some(resp);
            }
        }

        // Read and serve the file
        let content = match fs::read(&file_path).await {
            Ok(c) => c,
            Err(_) => return None,
        };

        let content_type = guess_content_type(&file_path);
        let mut resp = Response::new(StatusCode::OK);
        resp.headers.insert("content-type", content_type);
        resp.headers.insert("etag", &etag);
        resp.headers
            .insert("content-length", content.len().to_string());
        resp.body = content;

        Some(resp)
    }
}

fn guess_content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("txt") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HeaderMap, Method};

    #[tokio::test]
    async fn test_serve_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let server = StaticFileServer::new(dir.path());
        let req = Request {
            method: Method::Get,
            target: "/test.txt".into(),
            path: "/test.txt".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };

        let resp = server.serve(&req).await.unwrap();
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.body, b"hello world");
        assert!(resp.headers.get("etag").is_some());
    }

    #[tokio::test]
    async fn test_304_not_modified() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let server = StaticFileServer::new(dir.path());

        // First request to get ETag
        let req = Request {
            method: Method::Get,
            target: "/test.txt".into(),
            path: "/test.txt".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };
        let resp = server.serve(&req).await.unwrap();
        let etag = resp.headers.get("etag").unwrap().to_string();

        // Second request with If-None-Match
        let mut req2 = Request {
            method: Method::Get,
            target: "/test.txt".into(),
            path: "/test.txt".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };
        req2.headers.insert("if-none-match", &etag);
        let resp2 = server.serve(&req2).await.unwrap();
        assert_eq!(resp2.status, StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn test_reject_directory_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let server = StaticFileServer::new(dir.path());
        let req = Request {
            method: Method::Get,
            target: "/../etc/passwd".into(),
            path: "/../etc/passwd".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };

        let resp = server.serve(&req).await.unwrap();
        assert_eq!(resp.status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_nonexistent_file() {
        let dir = tempfile::tempdir().unwrap();
        let server = StaticFileServer::new(dir.path());
        let req = Request {
            method: Method::Get,
            target: "/nope.txt".into(),
            path: "/nope.txt".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };

        assert!(server.serve(&req).await.is_none());
    }
}

use crate::error::{NanowebError, Result};
use crate::http::request::Request;
use crate::http::response::Response;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct StaticFileServer {
    root: Arc<PathBuf>,
}

impl StaticFileServer {
    pub fn new(root: &str) -> Result<Self> {
        let root = PathBuf::from(root);
        if !root.is_dir() {
            return Err(NanowebError::Config(format!(
                "static directory does not exist: {}",
                root.display()
            )));
        }
        Ok(StaticFileServer {
            root: Arc::new(root),
        })
    }

    pub fn serve(&self, req: &Request) -> Response {
        let path = req.path.trim_start_matches('/');
        let full_path = self.root.join(path);

        let canonical_root = match self.root.canonicalize() {
            Ok(p) => p,
            Err(_) => return Response::new(500),
        };

        let canonical_path = match full_path.canonicalize() {
            Ok(p) => p,
            Err(_) => return Response::not_found(),
        };

        if !canonical_path.starts_with(&canonical_root) {
            return Response::new(403);
        }

        if !canonical_path.is_file() {
            return Response::not_found();
        }

        let metadata = match std::fs::metadata(&canonical_path) {
            Ok(m) => m,
            Err(_) => return Response::new(500),
        };

        let mtime = metadata
            .modified()
            .map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            })
            .unwrap_or(0);
        let size = metadata.len();
        let etag = format!("W/\"{}-{}\"", mtime, size);

        if let Some(if_none_match) = req.header("if-none-match") {
            if if_none_match.trim() == etag {
                return Response::new(304);
            }
        }

        let content_type = mime_from_path(&canonical_path);

        let body = match std::fs::read(&canonical_path) {
            Ok(b) => b,
            Err(_) => return Response::new(500),
        };

        let mut resp = Response::ok();
        resp.headers.insert("content-type".into(), content_type);
        resp.headers.insert("etag".into(), etag);
        resp.body = body;
        resp
    }
}

fn mime_from_path(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "txt" => "text/plain; charset=utf-8",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_serve_static_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("hello.txt"), "Hello, World!").unwrap();

        let server = StaticFileServer::new(dir.path().to_str().unwrap()).unwrap();
        let req = Request {
            method: crate::http::parser::Method::Get,
            path: "/hello.txt".into(),
            version: "HTTP/1.1".into(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            query: std::collections::HashMap::new(),
            remote_addr: None,
            request_id: "test".into(),
        };
        let resp = server.serve(&req);
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"Hello, World!");
    }

    #[test]
    fn test_directory_traversal_blocked() {
        let dir = TempDir::new().unwrap();
        let server = StaticFileServer::new(dir.path().to_str().unwrap()).unwrap();
        let req = Request {
            method: crate::http::parser::Method::Get,
            path: "/../../../etc/passwd".into(),
            version: "HTTP/1.1".into(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            query: std::collections::HashMap::new(),
            remote_addr: None,
            request_id: "test".into(),
        };
        let resp = server.serve(&req);
        assert!(resp.status == 403 || resp.status == 404);
    }
}

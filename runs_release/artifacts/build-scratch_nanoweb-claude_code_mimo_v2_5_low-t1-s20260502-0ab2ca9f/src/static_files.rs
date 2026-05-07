use std::path::{Path, PathBuf};

use crate::http::{Method, Request, Response};

#[derive(Clone)]
pub struct StaticFiles {
    root: PathBuf,
}

impl StaticFiles {
    pub fn new(root: &str) -> Self {
        let path = PathBuf::from(root);
        let root = path.canonicalize().unwrap_or(path);
        StaticFiles { root }
    }

    pub fn handle(&self, req: &Request, path: &str) -> Option<Response> {
        if req.method != Method::Get && req.method != Method::Head {
            return None;
        }

        let rel = path.trim_start_matches('/');
        let full = self.root.join(rel).canonicalize().ok()?;

        // Directory traversal prevention
        if !full.starts_with(&self.root) {
            return None;
        }

        if !full.is_file() {
            return None;
        }

        let meta = full.metadata().ok()?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let size = meta.len();

        let etag = format!("W/\"{}-{}\"", mtime, size);

        // If-None-Match check
        if let Some(if_none_match) = req.headers.get("if-none-match") {
            if if_none_match == etag {
                return Some(Response::not_modified());
            }
        }

        let content_type = guess_content_type(&full);
        let body = std::fs::read(&full).ok()?;

        let mut res = Response::new();
        res.status = crate::http::StatusCode::new(200);
        res.headers.insert("etag".into(), etag);
        res.headers.insert("content-type".into(), content_type);
        res.headers
            .insert("content-length".into(), body.len().to_string());
        res.body = body;
        Some(res)
    }
}

fn guess_content_type(path: &Path) -> String {
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
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Headers;

    #[test]
    fn test_serves_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let static_files = StaticFiles::new(dir.path().to_str().unwrap());
        let mut req = Request::new(Method::Get, "/test.txt".into());
        let resp = static_files.handle(&req, "/test.txt").unwrap();
        assert_eq!(resp.status.0, 200);
        assert_eq!(resp.body, b"hello world");
    }

    #[test]
    fn test_304_not_modified() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").unwrap();

        let static_files = StaticFiles::new(dir.path().to_str().unwrap());
        let mut req = Request::new(Method::Get, "/test.txt".into());
        let resp1 = static_files.handle(&req, "/test.txt").unwrap();
        let etag = resp1.headers.get("etag").unwrap().to_string();

        let mut req2 = Request::new(Method::Get, "/test.txt".into());
        req2.headers.insert("if-none-match".into(), etag);
        let resp2 = static_files.handle(&req2, "/test.txt").unwrap();
        assert_eq!(resp2.status.0, 304);
    }

    #[test]
    fn test_directory_traversal_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let static_files = StaticFiles::new(dir.path().to_str().unwrap());
        let req = Request::new(Method::Get, "/../../../etc/passwd".into());
        let resp = static_files.handle(&req, "/../../../etc/passwd");
        assert!(resp.is_none());
    }
}

use crate::error::NanowebError;
use crate::http::{Request, Response};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;

pub struct StaticFileServer {
    root: PathBuf,
}

impl StaticFileServer {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        // Canonicalize at creation time
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn serve(&self, req: &Request) -> Result<Response, NanowebError> {
        let path = req.path.strip_prefix('/').unwrap_or(&req.path);

        // Reject directory traversal
        if path.contains("..") || path.contains('\0') {
            return Ok(Response::new(400).with_body("Bad Request: path traversal detected"));
        }

        let file_path = self.root.join(path);

        // Canonicalize and check it's under root
        let canonical = match fs::canonicalize(&file_path).await {
            Ok(p) => p,
            Err(_) => return Ok(Response::new(404).with_body("Not Found")),
        };

        if !canonical.starts_with(&self.root) {
            return Ok(Response::new(403).with_body("Forbidden"));
        }

        let metadata = match fs::metadata(&canonical).await {
            Ok(m) => m,
            Err(_) => return Ok(Response::new(404).with_body("Not Found")),
        };

        if metadata.is_dir() {
            let index = canonical.join("index.html");
            if index.exists() {
                return self.serve_file(&index, req).await;
            }
            return Ok(Response::new(404).with_body("Not Found"));
        }

        self.serve_file(&canonical, req).await
    }

    async fn serve_file(&self, path: &Path, req: &Request) -> Result<Response, NanowebError> {
        let metadata = fs::metadata(path).await?;
        let mtime = metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let size = metadata.len();

        let mtime_secs = mtime
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let etag = format!(
            "W/\"{:x}-{:x}\"",
            Sha256::digest(format!("{mtime_secs}-{size}").as_bytes()),
            size
        );

        if let Some(if_none_match) = req.header("if-none-match") {
            if if_none_match == etag {
                return Ok(Response::new(304));
            }
        }

        let content_type = guess_content_type(path);
        let body = fs::read(path).await?;

        Ok(Response::ok()
            .with_header("content-type", content_type)
            .with_header("etag", &etag)
            .with_header("content-length", &body.len().to_string())
            .with_body(body))
    }
}

fn guess_content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("xml") => "application/xml",
        Some("txt") => "text/plain; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_test_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let html_path = dir.path().join("index.html");
        fs::write(&html_path, "<html><body>Hello</body></html>")
            .await
            .unwrap();

        let css_dir = dir.path().join("css");
        fs::create_dir(&css_dir).await.unwrap();
        fs::write(css_dir.join("style.css"), "body { color: red; }")
            .await
            .unwrap();

        dir
    }

    #[tokio::test]
    async fn test_serve_file() {
        let dir = setup_test_dir().await;
        let server = StaticFileServer::new(dir.path());
        let req = Request::new(
            crate::http::Method::Get,
            "/index.html".into(),
            "HTTP/1.1".into(),
        );
        let resp = server.serve(&req).await.unwrap();
        assert_eq!(resp.status, 200);
        assert!(resp.headers.iter().any(|(k, _)| k == "etag"));
    }

    #[tokio::test]
    async fn test_directory_traversal_blocked() {
        let dir = setup_test_dir().await;
        let server = StaticFileServer::new(dir.path());
        let req = Request::new(
            crate::http::Method::Get,
            "/../../../etc/passwd".into(),
            "HTTP/1.1".into(),
        );
        let resp = server.serve(&req).await.unwrap();
        assert_eq!(resp.status, 400);
    }

    #[tokio::test]
    async fn test_304_not_modified() {
        let dir = setup_test_dir().await;
        let server = StaticFileServer::new(dir.path());
        let req = Request::new(
            crate::http::Method::Get,
            "/index.html".into(),
            "HTTP/1.1".into(),
        );
        let resp = server.serve(&req).await.unwrap();
        let etag = resp
            .headers
            .iter()
            .find(|(k, _)| k == "etag")
            .unwrap()
            .1
            .clone();

        let mut req2 = Request::new(
            crate::http::Method::Get,
            "/index.html".into(),
            "HTTP/1.1".into(),
        );
        req2.headers.insert("if-none-match".into(), etag);
        let resp2 = server.serve(&req2).await.unwrap();
        assert_eq!(resp2.status, 304);
    }
}

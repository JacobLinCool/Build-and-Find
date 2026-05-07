use crate::error::NanowebError;
use crate::http::types::{Request, Response, Status};
use std::path::{Path, PathBuf};

pub struct StaticFileServer {
    pub root: PathBuf,
}

impl StaticFileServer {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        StaticFileServer { root: root.into() }
    }

    pub async fn serve(&self, req: &Request) -> Result<Response, NanowebError> {
        let path = req
            .params
            .get("path")
            .cloned()
            .unwrap_or_else(|| req.path.trim_start_matches('/').to_string());

        let file_path = self.resolve_path(&path)?;

        // Check file exists
        if !file_path.exists() {
            return Ok(Response::new(Status::NotFound).with_text("Not Found"));
        }

        if file_path.is_dir() {
            // Try index.html
            let index = file_path.join("index.html");
            if index.exists() {
                return self.serve_file(req, &index).await;
            }
            return Ok(Response::new(Status::NotFound).with_text("Directory listing not supported"));
        }

        self.serve_file(req, &file_path).await
    }

    fn resolve_path(&self, path: &str) -> Result<PathBuf, NanowebError> {
        // Prevent directory traversal
        let decoded = percent_encoding::percent_decode_str(path)
            .decode_utf8()
            .map_err(|_| NanowebError::BadRequest("invalid path encoding".to_string()))?;

        // Check for suspicious path components
        for component in decoded.split('/') {
            if component == ".." || component == "." {
                return Err(NanowebError::Forbidden);
            }
        }

        let candidate = self.root.join(decoded.as_ref());

        // Canonicalize and ensure within root
        let canonical_root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        let canonical_candidate = candidate.canonicalize().unwrap_or(candidate.clone());

        if !canonical_candidate.starts_with(&canonical_root) && candidate.exists() {
            return Err(NanowebError::Forbidden);
        }

        Ok(candidate)
    }

    async fn serve_file(&self, req: &Request, path: &Path) -> Result<Response, NanowebError> {
        let metadata = tokio::fs::metadata(path).await?;
        let modified = metadata.modified().ok();

        // Generate ETag from file size + modified time
        let etag = if let Some(mt) = modified {
            let duration = mt.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            format!("\"{}{}\"", duration.as_secs(), metadata.len())
        } else {
            format!("\"{}\"", metadata.len())
        };

        // Check If-None-Match
        if let Some(inm) = req.header("if-none-match") {
            if inm == etag || inm == "*" {
                return Ok(Response::new(Status::NotModified).set_header("etag", &etag));
            }
        }

        let content = tokio::fs::read(path).await?;
        let content_type = guess_content_type(path);

        Ok(Response::ok()
            .with_body(content)
            .set_header("content-type", content_type)
            .set_header("etag", &etag)
            .set_header("cache-control", "public, max-age=3600"))
    }
}

fn guess_content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
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

    #[tokio::test]
    async fn test_serve_file() {
        let dir = std::env::temp_dir().join("nanoweb_test_static");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("test.txt");
        std::fs::write(&file, b"hello world").unwrap();

        let server = StaticFileServer::new(&dir);
        let mut req = crate::http::types::Request::new(
            crate::http::types::Method::GET,
            "/test.txt".to_string(),
        );
        req.params
            .insert("path".to_string(), "test.txt".to_string());

        let resp = server.serve(&req).await.unwrap();
        assert_eq!(resp.status.code(), 200);
        assert_eq!(resp.body, b"hello world");
    }

    #[test]
    fn test_path_traversal_prevention() {
        let dir = std::env::temp_dir();
        let server = StaticFileServer::new(&dir);
        let result = server.resolve_path("../etc/passwd");
        assert!(result.is_err());
    }
}

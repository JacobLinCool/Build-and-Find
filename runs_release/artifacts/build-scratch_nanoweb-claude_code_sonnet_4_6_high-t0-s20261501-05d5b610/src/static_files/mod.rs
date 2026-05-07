use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::NanowebError;
use crate::http::{Request, Response};

pub struct StaticFileHandler {
    root: PathBuf,
}

impl StaticFileHandler {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub async fn handle(&self, req: &Request) -> Result<Response, NanowebError> {
        // Extract file path from request params or path
        let file_path = req
            .params
            .get("path")
            .map(|s| s.as_str())
            .unwrap_or_else(|| req.path.trim_start_matches('/'));

        self.serve_file(file_path, req).await
    }

    pub async fn serve_file(
        &self,
        file_path: &str,
        req: &Request,
    ) -> Result<Response, NanowebError> {
        // Sanitize path - reject directory traversal
        let safe_path = sanitize_path(file_path)?;
        let full_path = self.root.join(&safe_path);

        // Verify the resolved path is still under root
        let canonical_root =
            std::fs::canonicalize(&self.root).map_err(|_| NanowebError::NotFound)?;

        let canonical_path = match std::fs::canonicalize(&full_path) {
            Ok(p) => p,
            Err(_) => return Err(NanowebError::NotFound),
        };

        if !canonical_path.starts_with(&canonical_root) {
            return Err(NanowebError::Forbidden("Directory traversal denied".into()));
        }

        if canonical_path.is_dir() {
            // Try index.html
            let index = canonical_path.join("index.html");
            if index.exists() {
                return self.serve_path(&index, req).await;
            }
            return Err(NanowebError::NotFound);
        }

        self.serve_path(&canonical_path, req).await
    }

    async fn serve_path(&self, path: &Path, req: &Request) -> Result<Response, NanowebError> {
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|_| NanowebError::NotFound)?;

        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let size = metadata.len();

        // Compute weak ETag from mtime + size
        let etag = compute_etag(modified, size);

        // Check If-None-Match
        if let Some(inm) = req.headers.get("if-none-match") {
            let inm = inm.trim_matches('"');
            let our_etag = etag.trim_matches('"');
            if inm == our_etag || inm == format!("W/{}", etag) || inm == etag {
                let mut resp = Response::new(304);
                resp.headers.insert("ETag", etag);
                return Ok(resp);
            }
        }

        let data = tokio::fs::read(path)
            .await
            .map_err(|_| NanowebError::NotFound)?;

        let content_type = guess_content_type(path);

        let mut resp = Response::new(200);
        resp.headers.insert("Content-Type", content_type);
        resp.headers
            .insert("Content-Length", data.len().to_string());
        resp.headers.insert("ETag", etag);
        resp.headers.insert("Cache-Control", "public, max-age=3600");
        resp.body = data;

        Ok(resp)
    }
}

fn sanitize_path(path: &str) -> Result<PathBuf, NanowebError> {
    let path = path.trim_start_matches('/');

    // Reject null bytes
    if path.contains('\0') {
        return Err(NanowebError::Forbidden("Null byte in path".into()));
    }

    // Build path component by component, rejecting ..
    let mut result = PathBuf::new();
    for component in path.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                return Err(NanowebError::Forbidden("Directory traversal denied".into()));
            }
            c => result.push(c),
        }
    }

    Ok(result)
}

fn compute_etag(modified: SystemTime, size: u64) -> String {
    let secs = modified
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("W/\"{:x}-{:x}\"", secs, size)
}

fn guess_content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("pdf") => "application/pdf",
        Some("xml") => "application/xml",
        Some("txt") => "text/plain; charset=utf-8",
        Some("md") => "text/markdown",
        Some("webp") => "image/webp",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("ogg") => "audio/ogg",
        Some("mp3") => "audio/mpeg",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_path_rejects_traversal() {
        assert!(sanitize_path("../etc/passwd").is_err());
        assert!(sanitize_path("foo/../../../etc").is_err());
    }

    #[test]
    fn test_sanitize_path_normal() {
        let p = sanitize_path("css/main.css").unwrap();
        assert_eq!(p, PathBuf::from("css/main.css"));
    }

    #[test]
    fn test_sanitize_path_leading_slash() {
        let p = sanitize_path("/images/logo.png").unwrap();
        assert_eq!(p, PathBuf::from("images/logo.png"));
    }

    #[test]
    fn test_content_type_guessing() {
        assert_eq!(guess_content_type(Path::new("style.css")), "text/css");
        assert_eq!(
            guess_content_type(Path::new("app.js")),
            "application/javascript"
        );
        assert_eq!(
            guess_content_type(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            guess_content_type(Path::new("unknown.xyz")),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_compute_etag() {
        let t1 = SystemTime::UNIX_EPOCH;
        let etag = compute_etag(t1, 1024);
        assert!(etag.starts_with("W/\""));
        assert!(etag.ends_with('"'));
    }
}

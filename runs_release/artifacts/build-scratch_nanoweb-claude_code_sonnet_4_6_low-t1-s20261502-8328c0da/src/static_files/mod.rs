use crate::error::NanowebError;
use crate::http::types::{Request, Response, StatusCode};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub struct StaticFileServer {
    pub root: PathBuf,
}

impl StaticFileServer {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        StaticFileServer { root: root.into() }
    }

    pub async fn serve(&self, req: &Request, path_suffix: &str) -> Result<Response, NanowebError> {
        // Sanitize path - reject directory traversal
        let clean_path = sanitize_path(path_suffix)?;
        let file_path = self.root.join(&clean_path);

        // Verify file is within root
        let canonical_root = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        let canonical_file = match file_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                return Err(NanowebError::NotFound(format!(
                    "File not found: {}",
                    clean_path.display()
                )));
            }
        };

        if !canonical_file.starts_with(&canonical_root) {
            return Err(NanowebError::Forbidden("Directory traversal rejected".to_string()));
        }

        if !canonical_file.is_file() {
            return Err(NanowebError::NotFound(format!(
                "Not a file: {}",
                clean_path.display()
            )));
        }

        // Get file metadata for ETag
        let metadata = tokio::fs::metadata(&canonical_file).await?;
        let mtime = metadata
            .modified()
            .map_err(NanowebError::Io)?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let size = metadata.len();

        let etag = format!("W/\"{:x}-{:x}\"", mtime, size);

        // Check If-None-Match
        if let Some(client_etag) = req.headers.get("if-none-match") {
            if client_etag == etag {
                let resp = Response::new(StatusCode::NOT_MODIFIED)
                    .with_header("etag", &etag);
                return Ok(resp);
            }
        }

        // Read file
        let content = tokio::fs::read(&canonical_file).await?;
        let content_type = mime_type(&canonical_file);

        let resp = Response::new(StatusCode::OK)
            .with_body(content)
            .with_header("content-type", content_type)
            .with_header("etag", etag);

        Ok(resp)
    }
}

fn sanitize_path(path: &str) -> Result<PathBuf, NanowebError> {
    let path = path.trim_start_matches('/');

    // Reject paths with .. components
    for component in path.split('/') {
        if component == ".." || component == "." {
            return Err(NanowebError::Forbidden("Directory traversal rejected".to_string()));
        }
    }

    Ok(PathBuf::from(path))
}

fn mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(|s| s.to_str()) {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_path_ok() {
        let p = sanitize_path("/static/foo/bar.html").unwrap();
        assert_eq!(p, PathBuf::from("static/foo/bar.html"));
    }

    #[test]
    fn test_sanitize_path_traversal() {
        assert!(sanitize_path("/../etc/passwd").is_err());
        assert!(sanitize_path("/foo/../../../etc/passwd").is_err());
    }
}

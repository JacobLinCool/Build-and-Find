//! Static file serving with weak ETag, If-None-Match, and traversal protection.

use std::path::{Component, Path, PathBuf};

use crate::error::Result;
use crate::http::{Request, Response, StatusCode};

pub struct StaticFiles {
    pub root: PathBuf,
}

impl StaticFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Serve the file referenced by `relative_path` (the wildcard parameter).
    pub async fn serve(&self, req: &Request, relative_path: &str) -> Result<Response> {
        let safe_rel = match safe_join(&self.root, relative_path) {
            Some(p) => p,
            None => {
                return Ok(Response::new(StatusCode::FORBIDDEN).text("forbidden"));
            }
        };
        let metadata = match tokio::fs::metadata(&safe_rel).await {
            Ok(m) => m,
            Err(_) => return Ok(Response::new(StatusCode::NOT_FOUND).text("not found")),
        };
        if !metadata.is_file() {
            return Ok(Response::new(StatusCode::NOT_FOUND).text("not found"));
        }
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let etag = format!("W/\"{}-{}\"", mtime, metadata.len());
        if let Some(if_none_match) = req.header("if-none-match") {
            if if_none_match.split(',').any(|t| t.trim() == etag) {
                return Ok(Response::new(StatusCode::NOT_MODIFIED).header("etag", etag.clone()));
            }
        }
        let bytes = tokio::fs::read(&safe_rel).await?;
        let ct = guess_content_type(&safe_rel);
        Ok(Response::ok()
            .header("content-type", ct)
            .header("etag", etag)
            .body_bytes(bytes))
    }
}

/// Return the canonical absolute path for `root/rel` if it stays under `root`.
fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel_path = Path::new(rel);
    // Reject any component that is `..`, the root prefix `/`, etc.
    for c in rel_path.components() {
        match c {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
            _ => {}
        }
    }
    Some(root.join(rel_path))
}

fn guess_content_type(p: &Path) -> &'static str {
    match p
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
    {
        Some(ref ext) if ext == "html" || ext == "htm" => "text/html; charset=utf-8",
        Some(ref ext) if ext == "css" => "text/css; charset=utf-8",
        Some(ref ext) if ext == "js" => "application/javascript; charset=utf-8",
        Some(ref ext) if ext == "json" => "application/json",
        Some(ref ext) if ext == "txt" || ext == "md" => "text/plain; charset=utf-8",
        Some(ref ext) if ext == "png" => "image/png",
        Some(ref ext) if ext == "jpg" || ext == "jpeg" => "image/jpeg",
        Some(ref ext) if ext == "gif" => "image/gif",
        Some(ref ext) if ext == "svg" => "image/svg+xml",
        Some(ref ext) if ext == "ico" => "image/x-icon",
        Some(ref ext) if ext == "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal() {
        let root = PathBuf::from("/srv/www");
        assert!(safe_join(&root, "../etc/passwd").is_none());
        assert!(safe_join(&root, "/etc/passwd").is_none());
        assert!(safe_join(&root, "a/../b").is_none());
        assert!(safe_join(&root, "a/b.txt").is_some());
    }
}

use crate::http::{Request, Response, StatusCode};
use std::path::{Path, PathBuf};

pub struct StaticFiles {
    root: PathBuf,
}

impl StaticFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Serve a path relative to the configured root. Returns 404 on missing,
    /// 403 on traversal, 304 if If-None-Match matches the ETag.
    pub async fn serve(&self, req: &Request, rel: &str) -> Response {
        let rel = rel.trim_start_matches('/');
        // Reject traversal: any '..' component
        if rel.split('/').any(|s| s == "..") {
            return Response::new(StatusCode::FORBIDDEN).with_body(b"forbidden".to_vec());
        }
        let full = self.root.join(rel);
        // Canonicalize: ensure the resolved path starts with the canonical root.
        let canonical_root = match self.root.canonicalize() {
            Ok(p) => p,
            Err(_) => return Response::new(StatusCode::NOT_FOUND),
        };
        let canonical = match full.canonicalize() {
            Ok(p) => p,
            Err(_) => return Response::new(StatusCode::NOT_FOUND),
        };
        if !canonical.starts_with(&canonical_root) {
            return Response::new(StatusCode::FORBIDDEN).with_body(b"forbidden".to_vec());
        }
        let metadata = match tokio::fs::metadata(&canonical).await {
            Ok(m) => m,
            Err(_) => return Response::new(StatusCode::NOT_FOUND),
        };
        if !metadata.is_file() {
            return Response::new(StatusCode::NOT_FOUND);
        }
        let etag = compute_etag(&canonical, &metadata);
        if let Some(inm) = req.header("If-None-Match") {
            if inm.split(',').any(|t| t.trim() == etag) {
                let mut r = Response::new(StatusCode::NOT_MODIFIED);
                r.headers.insert("ETag", etag);
                return r;
            }
        }
        let bytes = match tokio::fs::read(&canonical).await {
            Ok(b) => b,
            Err(_) => return Response::new(StatusCode::INTERNAL_SERVER_ERROR),
        };
        let mut r = Response::new(StatusCode::OK);
        r.headers.insert("Content-Type", guess_mime(&canonical));
        r.headers.insert("ETag", etag);
        r.body = bytes;
        r
    }
}

fn compute_etag(_path: &Path, m: &std::fs::Metadata) -> String {
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("W/\"{:x}-{:x}\"", m.len(), mtime)
}

fn guess_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_guess() {
        assert_eq!(guess_mime(Path::new("a.html")), "text/html; charset=utf-8");
        assert_eq!(guess_mime(Path::new("a.png")), "image/png");
    }
}

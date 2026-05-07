use crate::http::{Request, Response, StatusCode};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct StaticFiles {
    pub root: PathBuf,
}

impl StaticFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub async fn serve(&self, req: &Request, sub_path: &str) -> Response {
        // Reject directory traversal: any segment equal to ".." is rejected;
        // also reject absolute or empty-leading constructs.
        if !is_safe_relative(sub_path) {
            let mut r = Response::new(StatusCode::FORBIDDEN);
            r.body = b"forbidden".to_vec();
            return r;
        }
        let candidate = self.root.join(sub_path);
        // Canonicalise to ensure the resolved path stays inside `root`.
        let root_canon = match self.root.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                return Response::new(StatusCode::INTERNAL_SERVER_ERROR);
            }
        };
        let real = match candidate.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                let mut r = Response::new(StatusCode::NOT_FOUND);
                r.body = b"not found".to_vec();
                return r;
            }
        };
        if !real.starts_with(&root_canon) {
            let mut r = Response::new(StatusCode::FORBIDDEN);
            r.body = b"forbidden".to_vec();
            return r;
        }
        let meta = match tokio::fs::metadata(&real).await {
            Ok(m) => m,
            Err(_) => {
                let mut r = Response::new(StatusCode::NOT_FOUND);
                r.body = b"not found".to_vec();
                return r;
            }
        };
        if !meta.is_file() {
            let mut r = Response::new(StatusCode::NOT_FOUND);
            r.body = b"not found".to_vec();
            return r;
        }
        let etag = compute_etag(&meta);
        if let Some(client_etag) = req.header("If-None-Match") {
            if etag_matches(client_etag, &etag) {
                let mut r = Response::new(StatusCode::NOT_MODIFIED);
                r.headers.insert("ETag", etag);
                return r;
            }
        }
        let bytes = match tokio::fs::read(&real).await {
            Ok(b) => b,
            Err(_) => {
                return Response::new(StatusCode::INTERNAL_SERVER_ERROR);
            }
        };
        let ct = guess_mime(&real);
        let mut r = Response::new(StatusCode::OK);
        r.headers.insert("Content-Type", ct);
        r.headers.insert("ETag", etag);
        r.body = bytes;
        r
    }
}

pub fn is_safe_relative(p: &str) -> bool {
    if p.starts_with('/') {
        return false;
    }
    for seg in p.split('/') {
        if seg == ".." {
            return false;
        }
        // also reject backslash sneaks
        if seg.contains('\\') {
            return false;
        }
    }
    true
}

pub fn compute_etag(meta: &std::fs::Metadata) -> String {
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("W/\"{:x}-{:x}\"", mtime, size)
}

fn etag_matches(client_value: &str, computed: &str) -> bool {
    client_value
        .split(',')
        .map(|s| s.trim())
        .any(|s| s == computed || s == "*")
}

fn guess_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript",
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
    fn rejects_traversal() {
        assert!(!is_safe_relative("../etc/passwd"));
        assert!(!is_safe_relative("/etc/passwd"));
        assert!(!is_safe_relative("a/../b"));
        assert!(is_safe_relative("a/b/c.txt"));
    }
}

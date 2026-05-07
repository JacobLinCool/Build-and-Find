//! Serve files from a directory on disk.
//!
//! * Weak ETag computed from `mtime:size`.
//! * `If-None-Match` honoured to produce `304 Not Modified`.
//! * Naive Content-Type guess based on file extension.

use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::NanowebError;
use crate::http::types::{Request, Response, StatusCode};

#[derive(Clone, Debug)]
pub struct StaticFilesConfig {
    pub root: PathBuf,
    pub index: String,
}

impl StaticFilesConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            index: "index.html".into(),
        }
    }
}

#[derive(Clone)]
pub struct StaticFiles {
    pub config: StaticFilesConfig,
}

impl StaticFiles {
    pub fn new(config: StaticFilesConfig) -> Self {
        Self { config }
    }

    /// Map a URL path to an on-disk path, rejecting traversal attempts.
    pub fn resolve(&self, url_path: &str) -> Result<PathBuf, NanowebError> {
        let rel = url_path.trim_start_matches('/');
        let mut target = self.config.root.clone();
        for seg in rel.split('/') {
            if seg.is_empty() || seg == "." {
                continue;
            }
            if seg == ".." {
                return Err(NanowebError::Other("path traversal rejected".into()));
            }
            target.push(seg);
        }
        if target.is_dir() {
            target.push(&self.config.index);
        }
        Ok(target)
    }

    /// Build a response for `req`.
    pub fn serve(&self, req: &Request) -> Result<Response, NanowebError> {
        let path = self.resolve(&req.path)?;
        if !path.exists() {
            return Ok(Response::status(StatusCode::NOT_FOUND));
        }
        let meta = std::fs::metadata(&path)?;
        let etag = weak_etag(&meta);
        if let Some(client_tag) = req.headers.get("if-none-match") {
            if etag_matches(client_tag, &etag) {
                let mut resp = Response::status(StatusCode::NOT_MODIFIED);
                resp.headers.set("etag", etag);
                return Ok(resp);
            }
        }
        let bytes = std::fs::read(&path)?;
        let mut resp = Response::new(StatusCode::OK);
        resp.headers.set("content-type", guess_mime(&path));
        resp.headers.set("content-length", bytes.len().to_string());
        resp.headers.set("etag", etag);
        resp.body = bytes;
        Ok(resp)
    }
}

fn weak_etag(meta: &Metadata) -> String {
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("W/\"{:x}-{:x}\"", mtime, size)
}

fn etag_matches(header: &str, our_tag: &str) -> bool {
    for candidate in header.split(',') {
        let c = candidate.trim();
        if c == "*" || c == our_tag {
            return true;
        }
    }
    false
}

fn guess_mime(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8".into(),
        "css" => "text/css; charset=utf-8".into(),
        "js" | "mjs" => "application/javascript".into(),
        "json" => "application/json".into(),
        "png" => "image/png".into(),
        "jpg" | "jpeg" => "image/jpeg".into(),
        "svg" => "image/svg+xml".into(),
        "gif" => "image/gif".into(),
        "txt" => "text/plain; charset=utf-8".into(),
        "woff2" => "font/woff2".into(),
        "ico" => "image/x-icon".into(),
        _ => "application/octet-stream".into(),
    }
}

/// Tiny utility — now_secs for testing etag generation.
#[allow(dead_code)]
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::Method;
    use std::io::Write;

    #[test]
    fn rejects_traversal() {
        let sf = StaticFiles::new(StaticFilesConfig::new("/tmp"));
        assert!(sf.resolve("/../etc/passwd").is_err());
    }

    #[test]
    fn guesses_mime_types() {
        assert_eq!(guess_mime(Path::new("a.css")), "text/css; charset=utf-8");
        assert_eq!(guess_mime(Path::new("a.bin")), "application/octet-stream");
    }

    #[test]
    fn serves_and_then_304s() {
        let dir = std::env::temp_dir().join(format!("nanoweb-sf-{}", now_secs()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hello.txt");
        {
            let mut f = std::fs::File::create(&file).unwrap();
            f.write_all(b"hi").unwrap();
        }
        let sf = StaticFiles::new(StaticFilesConfig::new(&dir));
        let mut req = Request::new(Method::Get, "/hello.txt");
        let resp = sf.serve(&req).unwrap();
        assert_eq!(resp.status, StatusCode::OK);
        let etag = resp.headers.get("etag").unwrap().to_string();

        req.headers.set("if-none-match", etag.clone());
        let resp2 = sf.serve(&req).unwrap();
        assert_eq!(resp2.status, StatusCode::NOT_MODIFIED);
    }
}

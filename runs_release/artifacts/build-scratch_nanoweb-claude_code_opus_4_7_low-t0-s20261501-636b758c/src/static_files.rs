//! Static file serving with weak ETag and traversal protection.

use crate::http::{Request, Response, StatusCode};
use crate::NanowebError;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

pub struct StaticFiles {
    pub root: PathBuf,
}

impl StaticFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn serve(&self, req: &Request, rel: &str) -> Result<Response, NanowebError> {
        let safe = match safe_join(&self.root, rel) {
            Some(p) => p,
            None => return Ok(Response::new(StatusCode::FORBIDDEN)),
        };
        let meta = match std::fs::metadata(&safe) {
            Ok(m) => m,
            Err(_) => return Ok(Response::new(StatusCode::NOT_FOUND)),
        };
        if !meta.is_file() {
            return Ok(Response::new(StatusCode::NOT_FOUND));
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let etag = format!("W/\"{}-{}\"", meta.len(), mtime);
        if let Some(inm) = req.header("if-none-match") {
            if inm == etag {
                let mut r = Response::new(StatusCode::NOT_MODIFIED);
                r.headers.insert("ETag", etag);
                return Ok(r);
            }
        }
        let body = std::fs::read(&safe).map_err(NanowebError::Io)?;
        let mut r = Response::new(StatusCode::OK);
        r.headers.insert("ETag", etag);
        r.headers
            .insert("Content-Type", guess_mime(&safe).to_string());
        r.body = body;
        Ok(r)
    }
}

pub fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut p = PathBuf::from(root);
    for c in Path::new(rel.trim_start_matches('/')).components() {
        match c {
            Component::Normal(s) => p.push(s),
            Component::CurDir => {}
            _ => return None,
        }
    }
    let canon_root = root.canonicalize().ok()?;
    let canon_target = p.canonicalize().ok()?;
    if canon_target.starts_with(canon_root) {
        Some(canon_target)
    } else {
        None
    }
}

pub fn guess_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css",
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
    use std::fs;

    #[test]
    fn rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let sf = StaticFiles::new(dir.path());
        let req = crate::http::Request {
            method: crate::http::Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: String::new(),
            version: "HTTP/1.1".into(),
            headers: crate::http::Headers::new(),
            body: vec![],
            params: Default::default(),
            remote_addr: None,
            request_id: String::new(),
            extensions: Default::default(),
        };
        let r = sf.serve(&req, "../etc/passwd").unwrap();
        assert_eq!(r.status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn serves_file_with_etag() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("hello.txt");
        fs::write(&p, b"hi").unwrap();
        let sf = StaticFiles::new(dir.path());
        let req = crate::http::Request {
            method: crate::http::Method::Get,
            target: "/hello.txt".into(),
            path: "/hello.txt".into(),
            query: String::new(),
            version: "HTTP/1.1".into(),
            headers: crate::http::Headers::new(),
            body: vec![],
            params: Default::default(),
            remote_addr: None,
            request_id: String::new(),
            extensions: Default::default(),
        };
        let r = sf.serve(&req, "hello.txt").unwrap();
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(r.body, b"hi");
        assert!(r.headers.get("etag").is_some());
    }
}

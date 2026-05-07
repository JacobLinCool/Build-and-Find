use crate::http::{Request, Response};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub struct StaticFileHandler {
    root: PathBuf,
}

impl StaticFileHandler {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn handle(&self, req: &Request) -> Option<Response> {
        let path_str = req.path.strip_prefix('/').unwrap_or(&req.path);
        let decoded = percent_decode(path_str);

        // Reject directory traversal
        if decoded.contains("..") || decoded.contains('\\') {
            return Some(Response::text(403, "Forbidden"));
        }

        let file_path = self.root.join(&decoded);

        // Canonicalize and verify it's within root
        let canonical = match file_path.canonicalize() {
            Ok(p) => p,
            Err(_) => return None,
        };
        let root_canonical = match self.root.canonicalize() {
            Ok(p) => p,
            Err(_) => return None,
        };
        if !canonical.starts_with(&root_canonical) {
            return Some(Response::text(403, "Forbidden"));
        }

        if !canonical.is_file() {
            return None;
        }

        let metadata = match std::fs::metadata(&canonical) {
            Ok(m) => m,
            Err(_) => return None,
        };

        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let size = metadata.len();

        // Generate weak ETag based on mtime + size
        let etag = format!("W/\"{mtime:x}-{size:x}\"");

        // Check If-None-Match
        if let Some(inm) = req.header("if-none-match") {
            if inm == etag {
                let mut resp = Response::new(304);
                resp.set_header("etag", &etag);
                return Some(resp);
            }
        }

        let content = match std::fs::read(&canonical) {
            Ok(c) => c,
            Err(_) => return None,
        };

        let mime = guess_mime(&canonical);
        let mut resp = Response::new(200);
        resp.set_header("content-type", mime);
        resp.set_header("content-length", content.len().to_string());
        resp.set_header("etag", etag);
        resp.body = content;
        Some(resp)
    }
}

fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("xx"),
                16,
            ) {
                result.push(byte as char);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn guess_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml",
        Some("pdf") => "application/pdf",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn reject_traversal() {
        let handler = StaticFileHandler::new("/tmp/static_test");
        let req = Request {
            method: crate::http::Method::Get,
            path: "/../../../etc/passwd".into(),
            query: None,
            version: "HTTP/1.1".into(),
            headers: HashMap::new(),
            body: Vec::new(),
            params: HashMap::new(),
            addr: "127.0.0.1:12345".parse().unwrap(),
            request_id: "test".into(),
        };
        let resp = handler.handle(&req).unwrap();
        assert_eq!(resp.status, 403);
    }
}

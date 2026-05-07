use crate::http::types::{Request, Response, StatusCode};
use std::path::{Path, PathBuf};

pub struct StaticFiles {
    root: PathBuf,
}

impl StaticFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        // Canonicalize root if it exists, otherwise use as-is
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        Self { root }
    }

    pub fn serve(&self, req: &Request) -> Option<Response> {
        let path = req.path.trim_start_matches('/');

        // Reject directory traversal
        if path.contains("..") || path.contains('\0') {
            return Some(
                Response::new(StatusCode::FORBIDDEN).with_body(b"Directory traversal forbidden"),
            );
        }

        // Prevent serving hidden files
        if path.starts_with('.') {
            return Some(Response::new(StatusCode::FORBIDDEN).with_body(b"Access denied"));
        }

        let file_path = self.root.join(path);

        // Verify the resolved path is still within root
        match std::fs::canonicalize(&file_path) {
            Ok(canonical) => {
                if !canonical.starts_with(&self.root) {
                    return Some(Response::new(StatusCode::FORBIDDEN).with_body(b"Access denied"));
                }
            }
            Err(_) => return Some(Response::not_found()),
        }

        let metadata = match std::fs::metadata(&file_path) {
            Ok(m) => m,
            Err(_) => return Some(Response::not_found()),
        };

        if metadata.is_dir() {
            // Try index.html
            let index = file_path.join("index.html");
            return match std::fs::read(&index) {
                Ok(contents) => {
                    let etag = compute_etag(&index);
                    Some(build_response(&contents, &etag, req))
                }
                Err(_) => Some(Response::not_found()),
            };
        }

        match std::fs::read(&file_path) {
            Ok(contents) => {
                let etag = compute_etag(&file_path);
                Some(build_response(&contents, &etag, req))
            }
            Err(_) => Some(Response::not_found()),
        }
    }
}

fn compute_etag(path: &Path) -> String {
    let metadata = std::fs::metadata(path).ok();
    let mtime = metadata
        .as_ref()
        .and_then(|m| m.modified().ok())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        })
        .unwrap_or(0);
    let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
    format!("W/\"{}-{}\"", mtime, size)
}

fn build_response(contents: &[u8], etag: &str, req: &Request) -> Response {
    // Check If-None-Match
    if let Some(if_none_match) = req.header("if-none-match") {
        if if_none_match.trim() == etag {
            return Response::new(StatusCode::NOT_MODIFIED);
        }
    }

    let content_type = guess_content_type(req.path.as_str());
    let mut resp = Response::ok();
    resp.body = contents.to_vec();
    resp.headers.insert("content-type", content_type);
    resp.headers.insert("etag", etag.to_string());
    resp.headers
        .insert("content-length", contents.len().to_string());
    resp
}

fn guess_content_type(path: &str) -> String {
    if path.ends_with(".html") || path.ends_with(".htm") {
        "text/html; charset=utf-8".into()
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8".into()
    } else if path.ends_with(".js") {
        "application/javascript; charset=utf-8".into()
    } else if path.ends_with(".json") {
        "application/json; charset=utf-8".into()
    } else if path.ends_with(".png") {
        "image/png".into()
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg".into()
    } else if path.ends_with(".gif") {
        "image/gif".into()
    } else if path.ends_with(".svg") {
        "image/svg+xml".into()
    } else if path.ends_with(".txt") {
        "text/plain; charset=utf-8".into()
    } else if path.ends_with(".xml") {
        "application/xml; charset=utf-8".into()
    } else {
        "application/octet-stream".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guess_type_html() {
        assert_eq!(guess_content_type("index.html"), "text/html; charset=utf-8");
    }

    #[test]
    fn guess_type_css() {
        assert_eq!(guess_content_type("style.css"), "text/css; charset=utf-8");
    }

    #[test]
    fn guess_type_js() {
        assert_eq!(
            guess_content_type("app.js"),
            "application/javascript; charset=utf-8"
        );
    }

    #[test]
    fn etag_format() {
        let etag = compute_etag(Path::new("/nonexistent"));
        assert!(etag.starts_with("W/\""));
        assert!(etag.ends_with('"'));
    }
}

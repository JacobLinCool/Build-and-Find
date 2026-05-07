use std::path::{Component, Path, PathBuf};

use tokio::fs;

use crate::error::{NanowebError, Result};
use crate::http::{Request, Response, StatusCode};

#[derive(Clone)]
pub struct StaticFiles {
    root: PathBuf,
}

impl StaticFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub async fn serve(&self, request: &Request, rel_path: &str) -> Result<Response> {
        if contains_traversal(rel_path) {
            return Ok(Response::text(StatusCode::BadRequest, "invalid path"));
        }
        let requested = self.root.join(rel_path.trim_start_matches('/'));
        let metadata = match fs::metadata(&requested).await {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Response::text(StatusCode::NotFound, "not found"));
            }
            Err(err) => return Err(NanowebError::from(err)),
        };
        if !metadata.is_file() {
            return Ok(Response::text(StatusCode::NotFound, "not found"));
        }
        let canonical_root = std::fs::canonicalize(&self.root)?;
        let canonical_file = std::fs::canonicalize(&requested)?;
        if !canonical_file.starts_with(&canonical_root) {
            return Ok(Response::text(StatusCode::BadRequest, "invalid path"));
        }

        let size = metadata.len();
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let etag = format!("W/\"{modified}-{size}\"");
        if request
            .header("if-none-match")
            .map(|value| etag_matches(value, &etag))
            .unwrap_or(false)
        {
            return Ok(Response::empty(StatusCode::NotModified).with_header("etag", etag));
        }

        let body = fs::read(&canonical_file).await?;
        let mime = mime_guess::from_path(&canonical_file).first_or_octet_stream();
        let mut response = Response::new(StatusCode::Ok);
        response.headers.insert("content-type", mime.essence_str());
        response.headers.insert("etag", etag);
        response.body = body;
        Ok(response)
    }
}

fn contains_traversal(path: &str) -> bool {
    Path::new(path)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
}

fn etag_matches(header: &str, etag: &str) -> bool {
    header
        .split(',')
        .map(|tag| tag.trim())
        .any(|tag| tag == etag || tag == "*")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HeaderMap;
    use tempfile::tempdir;

    #[tokio::test]
    async fn serves_file_and_supports_etag() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), "hello").unwrap();
        let static_files = StaticFiles::new(dir.path());
        let request = Request {
            method: crate::http::Method::Get,
            target: "/static/hello.txt".into(),
            path: "/static/hello.txt".into(),
            query: None,
            version: crate::http::Version::Http11,
            headers: HeaderMap::new(),
            body: vec![],
            params: Default::default(),
            remote_addr: None,
            request_id: "req".into(),
            session: None,
        };
        let response = static_files.serve(&request, "hello.txt").await.unwrap();
        assert_eq!(response.status, StatusCode::Ok);
        assert_eq!(response.body, b"hello");
    }
}

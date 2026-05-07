use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

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

    pub async fn serve(&self, request: Request) -> Result<Response> {
        let rel = request
            .path_param("path")
            .map(str::to_string)
            .unwrap_or_else(|| request.path.trim_start_matches('/').to_string());
        let path = sanitize_path(&self.root, &rel)?;
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(meta) if meta.is_file() => meta,
            _ => return Ok(Response::text(StatusCode::NOT_FOUND, "not found")),
        };
        let etag = weak_etag(&metadata);
        if request.header("if-none-match") == Some(etag.as_str()) {
            let mut response = Response::new(StatusCode::NOT_MODIFIED);
            response.set_header("ETag", etag);
            return Ok(response);
        }
        let body = tokio::fs::read(&path).await?;
        let mut response = Response::new(StatusCode::OK);
        response.body = body;
        response.set_header("ETag", etag);
        response.set_header(
            "Content-Type",
            mime_guess::from_path(&path)
                .first_or_octet_stream()
                .essence_str()
                .to_string(),
        );
        Ok(response)
    }
}

pub fn sanitize_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let mut out = PathBuf::from(root);
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return Err(NanowebError::Parse("directory traversal rejected".into())),
        }
    }
    Ok(out)
}

fn weak_etag(metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("W/\"{}-{}\"", modified, metadata.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_components() {
        assert!(sanitize_path(Path::new("/tmp"), "../secret").is_err());
    }

    #[test]
    fn accepts_nested_file() {
        assert_eq!(
            sanitize_path(Path::new("/tmp/root"), "a/b.txt").unwrap(),
            PathBuf::from("/tmp/root/a/b.txt")
        );
    }
}

use crate::http::{Request, Response, StatusCode};
use std::path::{Component, Path, PathBuf};
use tokio::fs;

#[derive(Clone)]
pub struct StaticFiles {
    root: PathBuf,
}

impl StaticFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub async fn serve(&self, req: Request, prefix: &str) -> Response {
        let rel = req
            .path
            .strip_prefix(prefix)
            .unwrap_or(&req.path)
            .trim_start_matches('/');
        if rel.is_empty()
            || Path::new(rel)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Response::text(StatusCode::Forbidden, "forbidden");
        }
        let path = self.root.join(rel);
        let Ok(meta) = fs::metadata(&path).await else {
            return Response::text(StatusCode::NotFound, "not found");
        };
        if !meta.is_file() {
            return Response::text(StatusCode::Forbidden, "forbidden");
        }
        let etag = weak_etag(&meta);
        if req.header("if-none-match") == Some(etag.as_str()) {
            let mut res = Response::new(StatusCode::NotModified);
            res.header("ETag", etag);
            return res;
        }
        match fs::read(&path).await {
            Ok(body) => {
                let mut res = Response::new(StatusCode::Ok);
                res.header("ETag", etag);
                if let Some(mime) = mime_guess::from_path(&path).first() {
                    res.header("Content-Type", mime.essence_str());
                }
                res.body = body;
                res
            }
            Err(_) => Response::text(StatusCode::NotFound, "not found"),
        }
    }
}

fn weak_etag(meta: &std::fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("W/\"{}-{}\"", meta.len(), mtime)
}

#[cfg(test)]
mod tests {
    #[test]
    fn construct() {
        let _ = super::StaticFiles::new(".");
    }
}

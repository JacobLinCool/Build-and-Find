use crate::http::{Request, Response};
use percent_encoding::percent_decode_str;
use std::{
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};
use tokio::fs;

#[derive(Clone)]
pub struct StaticFiles {
    root: PathBuf,
}

impl StaticFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub async fn serve(&self, request: &Request, wildcard_param: &str) -> Response {
        let Some(rel) = request.params.get(wildcard_param) else {
            return Response::text(404, "not found");
        };
        let decoded = match percent_decode_str(rel).decode_utf8() {
            Ok(s) => s,
            Err(_) => return Response::text(400, "bad path"),
        };
        if decoded
            .split('/')
            .any(|segment| segment == ".." || segment.contains('\\'))
        {
            return Response::text(403, "forbidden");
        }
        let mut clean = PathBuf::new();
        for comp in Path::new(decoded.as_ref()).components() {
            match comp {
                Component::Normal(part) => clean.push(part),
                Component::CurDir => {}
                _ => return Response::text(403, "forbidden"),
            }
        }
        let path = self.root.join(clean);
        let meta = match fs::metadata(&path).await {
            Ok(meta) if meta.is_file() => meta,
            _ => return Response::text(404, "not found"),
        };
        let etag = weak_etag(&meta);
        if request.header("if-none-match") == Some(etag.as_str()) {
            let mut response = Response::new(304);
            response.set_header("ETag", etag);
            return response;
        }
        let body = match fs::read(&path).await {
            Ok(body) => body,
            Err(_) => return Response::text(404, "not found"),
        };
        let mut response = Response::new(200);
        response.set_header(
            "Content-Type",
            mime_guess::from_path(&path)
                .first_or_octet_stream()
                .essence_str(),
        );
        response.set_header("ETag", etag);
        response.body = body;
        response
    }
}

fn weak_etag(meta: &std::fs::Metadata) -> String {
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or_default();
    format!("W/\"{}-{}\"", modified, meta.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;

    #[tokio::test]
    async fn rejects_traversal() {
        let files = StaticFiles::new(".");
        let mut req = Request::new(Method::Get, "/static/../Cargo.toml");
        req.params.insert("path".into(), "../Cargo.toml".into());
        assert_eq!(files.serve(&req, "path").await.status, 403);
    }
}

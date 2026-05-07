use crate::{
    error::{NanowebError, Result},
    http::types::{Request, Response, StatusCode},
    middleware::Handler,
};
use async_trait::async_trait;
use mime_guess::from_path;
use std::{fs, path::PathBuf, sync::Arc, time::UNIX_EPOCH};

#[derive(Clone)]
pub struct StaticFileHandler {
    root: Arc<PathBuf>,
}

impl StaticFileHandler {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Arc::new(root.into()),
        }
    }

    fn resolve_path(&self, requested: &str) -> Result<PathBuf> {
        let root = fs::canonicalize(&*self.root)?;
        let clean = requested.trim_start_matches('/');
        let candidate = root.join(clean);
        let candidate = fs::canonicalize(&candidate)
            .map_err(|_| NanowebError::Other("not found".to_string()))?;
        if !candidate.starts_with(&root) {
            return Err(NanowebError::Other("path traversal rejected".to_string()));
        }
        Ok(candidate)
    }
}

#[async_trait]
impl Handler for StaticFileHandler {
    async fn handle(
        &self,
        req: Request,
        _ctx: std::sync::Arc<crate::app::AppState>,
    ) -> Result<Response> {
        let requested = req
            .param("path")
            .or_else(|| req.path.strip_prefix('/'))
            .unwrap_or("");
        let file_path = self.resolve_path(requested)?;
        let metadata = fs::metadata(&file_path)?;
        if metadata.is_dir() {
            return Ok(Response::empty(StatusCode::NotFound));
        }
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        let modified = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
        let etag = format!("W/\"{}-{}\"", modified.as_secs(), metadata.len());
        if req
            .header("if-none-match")
            .map(|header| {
                header
                    .split(',')
                    .any(|candidate| candidate.trim() == etag || candidate.trim() == "*")
            })
            .unwrap_or(false)
        {
            return Ok(Response::empty(StatusCode::NotModified).with_header("etag", etag));
        }
        let bytes = tokio::fs::read(&file_path).await?;
        let mut response = Response::new(StatusCode::Ok);
        response.body = bytes;
        response.set_header(
            "content-type",
            from_path(&file_path)
                .first_or_octet_stream()
                .essence_str()
                .to_string(),
        );
        response.set_header("etag", etag);
        response.set_header("cache-control", "private, max-age=0");
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::StaticFileHandler;
    use crate::{
        app::AppState,
        http::types::{Method, Request, Version},
        middleware::Handler,
    };
    use std::{collections::HashMap, sync::Arc};
    use tempfile::tempdir;

    #[tokio::test]
    async fn serves_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), "hi").unwrap();
        let handler = StaticFileHandler::new(dir.path());
        let req = Request {
            method: Method::GET,
            target: "/static/hello.txt".to_string(),
            path: "/static/hello.txt".to_string(),
            query: None,
            version: Version::Http11,
            headers: HashMap::new(),
            body: Vec::new(),
            params: [("path".to_string(), "hello.txt".to_string())]
                .into_iter()
                .collect(),
            request_id: "req".to_string(),
            remote_addr: None,
            route_pattern: None,
            connection_close: false,
        };
        let response = handler
            .handle(req, Arc::new(AppState::test()))
            .await
            .unwrap();
        assert_eq!(response.status.as_u16(), 200);
    }
}

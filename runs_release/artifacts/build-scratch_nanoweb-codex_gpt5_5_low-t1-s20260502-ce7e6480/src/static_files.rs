use crate::error::{NanowebError, Result};
use crate::http::{Request, Response, StatusCode};
use std::path::{Path, PathBuf};
use tokio::fs;

pub async fn serve(
    root: impl AsRef<Path>,
    req: &Request,
    wildcard_param: &str,
) -> Result<Response> {
    let rel = req
        .params
        .get(wildcard_param)
        .map(String::as_str)
        .unwrap_or("");
    if rel.split('/').any(|s| s == "..") {
        return Ok(Response::new(StatusCode::FORBIDDEN, "forbidden"));
    }
    let path: PathBuf = root.as_ref().join(rel.trim_start_matches('/'));
    let meta = fs::metadata(&path)
        .await
        .map_err(|_| NanowebError::Other("not found".into()))?;
    if !meta.is_file() {
        return Ok(Response::new(StatusCode::NOT_FOUND, "not found"));
    }
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let etag = format!("W/\"{}-{}\"", modified, meta.len());
    if req.header("if-none-match") == Some(etag.as_str()) {
        let mut r = Response::new(StatusCode::NOT_MODIFIED, Vec::new());
        r.headers.insert("etag".into(), etag);
        return Ok(r);
    }
    let body = fs::read(&path).await?;
    let mut r = Response::new(StatusCode::OK, body);
    r.headers.insert("etag".into(), etag);
    r.headers.insert(
        "content-type".into(),
        mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string(),
    );
    Ok(r)
}

#[cfg(test)]
mod tests {
    #[test]
    fn traversal_segments_are_detectable() {
        assert!("a/../b".split('/').any(|s| s == ".."));
    }
}

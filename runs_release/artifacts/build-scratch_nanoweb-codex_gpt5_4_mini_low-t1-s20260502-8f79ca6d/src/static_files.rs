use crate::http::Response;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

pub fn serve(base: &Path, target: &str, if_none_match: Option<&str>) -> Response {
    let rel = target.trim_start_matches('/');
    if rel.contains("..") {
        return Response::text(400, "bad path");
    }
    let path = base.join(rel);
    let meta = match fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => return Response::text(404, "not found"),
    };
    if meta.is_dir() {
        return Response::text(404, "not found");
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let etag = format!("W/\"{}-{}\"", mtime, meta.len());
    if if_none_match == Some(etag.as_str()) {
        let mut r = Response::new(304, Vec::new());
        r.headers.insert("etag".into(), etag);
        return r;
    }
    let body = fs::read(&path).unwrap_or_default();
    let mut r = Response::new(200, body);
    r.headers.insert("etag".into(), etag);
    r
}

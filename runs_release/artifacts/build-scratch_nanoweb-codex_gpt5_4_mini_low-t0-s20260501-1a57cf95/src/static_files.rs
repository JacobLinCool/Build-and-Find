use crate::error::NanowebError;
use crate::http::types::Response;
use std::path::Path;

pub fn serve(
    root: &Path,
    target: &str,
    if_none_match: Option<&str>,
) -> Result<Response, NanowebError> {
    let rel = target.trim_start_matches('/');
    let path = root.join(rel);
    let canon = path
        .canonicalize()
        .map_err(|_| NanowebError::parse("not found"))?;
    let root_canon = root.canonicalize().map_err(NanowebError::Io)?;
    if !canon.starts_with(root_canon) {
        return Err(NanowebError::parse("traversal rejected"));
    }
    let meta = std::fs::metadata(&canon)?;
    let etag = format!(
        "W/\"{}-{}\"",
        meta.len(),
        meta.modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    if if_none_match == Some(etag.as_str()) {
        return Ok(Response::new(304, Vec::new()));
    }
    let body = std::fs::read(canon)?;
    let mut r = Response::new(200, body);
    r.headers.insert("etag".into(), etag);
    r.headers.insert(
        "content-type".into(),
        mime_guess::from_path(rel)
            .first_or_octet_stream()
            .to_string(),
    );
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traversal_rejected() {
        let root = tempfile::tempdir().unwrap();
        assert!(serve(root.path(), "../x", None).is_err());
    }
}

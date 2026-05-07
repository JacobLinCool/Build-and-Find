//! JSON body helpers wrapping `serde_json`.

use crate::error::NanowebError;
use crate::http::types::{Request, Response};

pub type JsonValue = serde_json::Value;

/// Parse the request body as JSON.
pub fn parse_json(req: &Request) -> Result<JsonValue, NanowebError> {
    if req.body.is_empty() {
        return Err(NanowebError::Body("empty json body".into()));
    }
    serde_json::from_slice(&req.body).map_err(|e| NanowebError::Body(format!("json: {}", e)))
}

/// Serialise any `Serialize` value and wrap it in a 200 response.
pub fn render_json<T: serde::Serialize>(value: &T) -> Result<Response, NanowebError> {
    let bytes =
        serde_json::to_vec(value).map_err(|e| NanowebError::Body(format!("json encode: {}", e)))?;
    Ok(Response::json_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::Method;
    use serde_json::json;

    #[test]
    fn roundtrips_json() {
        let mut req = Request::new(Method::Post, "/");
        req.body = br#"{"n":1}"#.to_vec();
        let v = parse_json(&req).unwrap();
        assert_eq!(v["n"], json!(1));
    }

    #[test]
    fn empty_body_is_error() {
        let req = Request::new(Method::Post, "/");
        assert!(parse_json(&req).is_err());
    }

    #[test]
    fn render_json_sets_content_type() {
        let resp = render_json(&json!({"ok": true})).unwrap();
        assert_eq!(resp.headers.get("content-type"), Some("application/json"));
    }
}

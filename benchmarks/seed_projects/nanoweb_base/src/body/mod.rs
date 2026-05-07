//! Typed body helpers.

pub mod form;
pub mod json;
pub mod multipart;

pub use form::parse_form;
pub use json::{parse_json, render_json, JsonValue};
pub use multipart::{parse_multipart, MultipartField};

use crate::http::types::Request;

/// Best-effort inspection of the `Content-Type` header.
pub fn content_type(req: &Request) -> Option<&str> {
    req.headers.get("content-type")
}

/// Extract just the mime type (ignoring parameters like `charset=utf-8`).
pub fn mime_type(req: &Request) -> Option<&str> {
    content_type(req).and_then(|v| v.split(';').next()).map(|s| s.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::Method;

    #[test]
    fn mime_type_trims_params() {
        let mut r = Request::new(Method::Post, "/");
        r.headers.set("content-type", "application/json; charset=utf-8");
        assert_eq!(mime_type(&r), Some("application/json"));
    }
}

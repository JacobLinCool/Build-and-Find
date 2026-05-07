use crate::http::Response;
pub fn healthz() -> Response {
    Response::text(200, "ok")
}
pub fn readyz(ready: bool) -> Response {
    if ready {
        Response::text(200, "ok")
    } else {
        Response::text(503, "not ready")
    }
}

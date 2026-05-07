use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::http::types::{Request, Response, StatusCode};

pub struct HealthState {
    pub ready: AtomicBool,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
        }
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }
}

impl Default for HealthState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn healthz_handler(_req: &Request) -> Response {
    Response::ok()
}

pub fn readyz_handler(state: &Arc<HealthState>, _req: &Request) -> Response {
    if state.ready.load(Ordering::SeqCst) {
        Response::ok()
    } else {
        let mut resp = Response::new(StatusCode(503));
        resp.body = b"not ready".to_vec();
        resp.headers
            .insert("content-type", "text/plain; charset=utf-8");
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthz_always_ok() {
        let req = Request::default();
        let resp = healthz_handler(&req);
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn readyz_when_not_ready() {
        let state = Arc::new(HealthState::new());
        let req = Request::default();
        let resp = readyz_handler(&state, &req);
        assert_eq!(resp.status, StatusCode(503));
    }

    #[test]
    fn readyz_when_ready() {
        let state = Arc::new(HealthState::new());
        state.set_ready(true);
        let req = Request::default();
        let resp = readyz_handler(&state, &req);
        assert_eq!(resp.status, StatusCode::OK);
    }
}

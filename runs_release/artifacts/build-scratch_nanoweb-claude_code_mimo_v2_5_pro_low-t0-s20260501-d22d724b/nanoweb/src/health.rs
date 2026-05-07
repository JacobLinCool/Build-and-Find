use crate::http::Response;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct HealthState {
    ready: Arc<AtomicBool>,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }

    pub fn healthz(&self) -> Response {
        Response::ok()
            .with_header("content-type", "application/json")
            .with_body(b"{\"status\":\"ok\"}".to_vec())
    }

    pub fn readyz(&self) -> Response {
        if self.is_ready() {
            Response::ok()
                .with_header("content-type", "application/json")
                .with_body(b"{\"status\":\"ready\"}".to_vec())
        } else {
            Response::new(503)
                .with_header("content-type", "application/json")
                .with_body(b"{\"status\":\"not_ready\"}".to_vec())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_healthz() {
        let state = HealthState::new();
        let resp = state.healthz();
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn test_readyz_not_ready() {
        let state = HealthState::new();
        let resp = state.readyz();
        assert_eq!(resp.status, 503);
    }

    #[test]
    fn test_readyz_ready() {
        let state = HealthState::new();
        state.set_ready(true);
        let resp = state.readyz();
        assert_eq!(resp.status, 200);
    }
}

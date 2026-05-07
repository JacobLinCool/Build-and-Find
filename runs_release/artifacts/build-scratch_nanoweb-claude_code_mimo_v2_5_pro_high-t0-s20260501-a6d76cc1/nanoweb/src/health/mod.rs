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
            ready: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }

    pub fn healthz(&self) -> Response {
        Response::text(200, "OK")
    }

    pub fn readyz(&self) -> Response {
        if self.is_ready() {
            Response::text(200, "OK")
        } else {
            Response::text(503, "Service Unavailable")
        }
    }
}

impl Default for HealthState {
    fn default() -> Self {
        Self::new()
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
    fn test_readyz() {
        let state = HealthState::new();
        assert_eq!(state.readyz().status, 200);
        state.set_ready(false);
        assert_eq!(state.readyz().status, 503);
    }
}

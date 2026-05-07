use crate::http::types::Response;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct HealthState {
    ready: AtomicBool,
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

    pub fn healthz(&self) -> Response {
        Response::text(200, "OK")
    }

    pub fn readyz(&self) -> Response {
        if self.ready.load(Ordering::SeqCst) {
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
    fn test_healthz_always_ok() {
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

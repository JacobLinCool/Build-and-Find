use crate::http::{Response, StatusCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Health check state shared across the server.
#[derive(Clone)]
pub struct HealthState {
    /// Whether the server is ready to accept traffic.
    ready: Arc<AtomicBool>,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Mark the server as ready.
    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    /// Check if the server is ready.
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }

    /// Handle /healthz - returns 200 if server is up.
    pub fn healthz(&self) -> Response {
        Response::text(StatusCode::OK, "OK")
    }

    /// Handle /readyz - returns 200 if server is ready to accept traffic.
    pub fn readyz(&self) -> Response {
        if self.is_ready() {
            Response::text(StatusCode::OK, "OK")
        } else {
            Response::text(StatusCode::INTERNAL_SERVER_ERROR, "Not Ready")
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
        let health = HealthState::new();
        let resp = health.healthz();
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn test_readyz_not_ready() {
        let health = HealthState::new();
        let resp = health.readyz();
        assert_eq!(resp.status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_readyz_ready() {
        let health = HealthState::new();
        health.set_ready(true);
        let resp = health.readyz();
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn test_readyz_shutdown() {
        let health = HealthState::new();
        health.set_ready(true);
        assert_eq!(health.readyz().status, StatusCode::OK);
        health.set_ready(false);
        assert_eq!(health.readyz().status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}

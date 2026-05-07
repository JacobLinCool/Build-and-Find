use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::http::{Response, StatusCode};

#[derive(Clone)]
pub struct HealthState {
    live: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            live: Arc::new(AtomicBool::new(true)),
            ready: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn set_ready(&self, value: bool) {
        self.ready.store(value, Ordering::SeqCst);
    }

    pub fn set_live(&self, value: bool) {
        self.live.store(value, Ordering::SeqCst);
    }

    pub fn healthz(&self) -> Response {
        if self.live.load(Ordering::SeqCst) {
            Response::text(StatusCode::Ok, "ok")
        } else {
            Response::text(StatusCode::ServiceUnavailable, "unhealthy")
        }
    }

    pub fn readyz(&self) -> Response {
        if self.live.load(Ordering::SeqCst) && self.ready.load(Ordering::SeqCst) {
            Response::text(StatusCode::Ok, "ready")
        } else {
            Response::text(StatusCode::ServiceUnavailable, "not ready")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_flips() {
        let health = HealthState::new();
        assert_eq!(health.readyz().status, StatusCode::Ok);
        health.set_ready(false);
        assert_eq!(health.readyz().status, StatusCode::ServiceUnavailable);
    }
}

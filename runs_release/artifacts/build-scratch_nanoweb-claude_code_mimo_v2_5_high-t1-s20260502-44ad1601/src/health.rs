use crate::http::response::Response;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct HealthState {
    started: Arc<AtomicBool>,
    shutting_down: Arc<AtomicBool>,
}

impl HealthState {
    pub fn new() -> Self {
        HealthState {
            started: Arc::new(AtomicBool::new(false)),
            shutting_down: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn mark_started(&self) {
        self.started.store(true, Ordering::SeqCst);
    }

    pub fn mark_shutting_down(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
    }

    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }

    pub fn healthz(&self) -> Response {
        Response::text("OK")
    }

    pub fn readyz(&self) -> Response {
        if self.is_started() && !self.is_shutting_down() {
            Response::text("OK")
        } else {
            let mut resp = Response::new(503);
            resp.body = b"Not Ready".to_vec();
            resp
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
    fn test_health_check() {
        let state = HealthState::new();
        assert_eq!(state.healthz().status, 200);
        assert_eq!(state.readyz().status, 503);

        state.mark_started();
        assert_eq!(state.readyz().status, 200);

        state.mark_shutting_down();
        assert_eq!(state.readyz().status, 503);
    }
}

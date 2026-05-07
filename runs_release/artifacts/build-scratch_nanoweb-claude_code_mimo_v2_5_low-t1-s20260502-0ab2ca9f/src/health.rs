use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::http::{Request, Response};

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

    pub fn is_ready(&self) -> bool {
        self.started.load(Ordering::SeqCst) && !self.shutting_down.load(Ordering::SeqCst)
    }

    pub fn healthz(&self, _req: &Request) -> Response {
        let mut res = Response::new();
        res.status = crate::http::StatusCode::new(200);
        res.set_text("OK");
        res
    }

    pub fn readyz(&self, _req: &Request) -> Response {
        let mut res = Response::new();
        if self.is_ready() {
            res.status = crate::http::StatusCode::new(200);
            res.set_text("OK");
        } else {
            res.status = crate::http::StatusCode::new(503);
            res.set_text("Not Ready");
        }
        res
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
    fn test_health_endpoints() {
        let state = HealthState::new();
        let req = Request::new(crate::http::Method::Get, "/healthz".into());

        // Before startup
        let res = state.healthz(&req);
        assert_eq!(res.status.0, 200);

        let res = state.readyz(&req);
        assert_eq!(res.status.0, 503);

        // After startup
        state.mark_started();
        let res = state.readyz(&req);
        assert_eq!(res.status.0, 200);

        // During shutdown
        state.mark_shutting_down();
        let res = state.readyz(&req);
        assert_eq!(res.status.0, 503);
    }
}

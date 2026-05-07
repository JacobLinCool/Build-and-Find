use crate::http::types::{Response, StatusCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct HealthState {
    pub ready: Arc<AtomicBool>,
    pub shutting_down: Arc<AtomicBool>,
}

impl HealthState {
    pub fn new() -> Self {
        HealthState {
            ready: Arc::new(AtomicBool::new(false)),
            shutting_down: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    pub fn set_shutting_down(&self, value: bool) {
        self.shutting_down.store(value, Ordering::SeqCst);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst) && !self.shutting_down.load(Ordering::SeqCst)
    }

    pub fn is_alive(&self) -> bool {
        true // Always alive unless process is dead
    }

    pub fn healthz_response(&self) -> Response {
        if self.is_alive() {
            Response::text("OK").with_header("content-type", "text/plain")
        } else {
            let mut resp = Response::new(StatusCode::INTERNAL_SERVER_ERROR);
            resp.body = b"Not healthy".to_vec();
            resp
        }
    }

    pub fn readyz_response(&self) -> Response {
        if self.is_ready() {
            Response::text("OK").with_header("content-type", "text/plain")
        } else {
            let mut resp = Response::new(StatusCode(503));
            resp.body = b"Not ready".to_vec();
            resp
        }
    }
}

impl Default for HealthState {
    fn default() -> Self {
        Self::new()
    }
}

use crate::http::{Response, StatusCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct Health {
    pub up: Arc<AtomicBool>,
    pub ready: Arc<AtomicBool>,
}

impl Health {
    pub fn new() -> Self {
        Self {
            up: Arc::new(AtomicBool::new(true)),
            ready: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn set_ready(&self, v: bool) {
        self.ready.store(v, Ordering::SeqCst);
    }
    pub fn shutdown(&self) {
        self.ready.store(false, Ordering::SeqCst);
    }

    pub fn healthz(&self) -> Response {
        if self.up.load(Ordering::SeqCst) {
            Response::text("ok")
        } else {
            Response::new(StatusCode::SERVICE_UNAVAILABLE).with_body(b"down".to_vec())
        }
    }

    pub fn readyz(&self) -> Response {
        if self.ready.load(Ordering::SeqCst) {
            Response::text("ready")
        } else {
            Response::new(StatusCode::SERVICE_UNAVAILABLE).with_body(b"not ready".to_vec())
        }
    }
}

impl Default for Health {
    fn default() -> Self {
        Self::new()
    }
}

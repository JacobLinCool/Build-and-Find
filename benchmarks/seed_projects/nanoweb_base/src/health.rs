//! Health endpoints.
//!
//! * `/healthz` — liveness, flips to `OK` as soon as the server has
//!   started handling connections.
//! * `/readyz` — readiness, flips to `OK` after startup completes, and
//!   back to `503` during graceful shutdown.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::http::types::{Response, StatusCode};

/// Shared handle referenced by both the server and the health route.
#[derive(Clone, Default)]
pub struct HealthState {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    live: AtomicBool,
    ready: AtomicBool,
}

impl HealthState {
    pub fn new() -> Self {
        HealthState::default()
    }

    pub fn mark_live(&self) {
        self.inner.live.store(true, Ordering::SeqCst);
    }

    pub fn mark_ready(&self) {
        self.inner.ready.store(true, Ordering::SeqCst);
    }

    pub fn mark_shutdown(&self) {
        self.inner.ready.store(false, Ordering::SeqCst);
    }

    pub fn is_live(&self) -> bool {
        self.inner.live.load(Ordering::SeqCst)
    }

    pub fn is_ready(&self) -> bool {
        self.inner.ready.load(Ordering::SeqCst)
    }

    pub fn healthz(&self) -> Response {
        if self.is_live() {
            let mut r = Response::new(StatusCode::OK);
            r.headers.set("content-type", "text/plain");
            r.body = b"ok\n".to_vec();
            r.headers.set("content-length", r.body.len().to_string());
            r
        } else {
            Response::status(StatusCode::SERVICE_UNAVAILABLE)
        }
    }

    pub fn readyz(&self) -> Response {
        if self.is_ready() {
            let mut r = Response::new(StatusCode::OK);
            r.headers.set("content-type", "text/plain");
            r.body = b"ready\n".to_vec();
            r.headers.set("content-length", r.body.len().to_string());
            r
        } else {
            Response::status(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_unhealthy_and_becomes_healthy() {
        let h = HealthState::new();
        assert_eq!(h.healthz().status, StatusCode::SERVICE_UNAVAILABLE);
        h.mark_live();
        assert_eq!(h.healthz().status, StatusCode::OK);
    }

    #[test]
    fn ready_flips_back_on_shutdown() {
        let h = HealthState::new();
        h.mark_ready();
        assert_eq!(h.readyz().status, StatusCode::OK);
        h.mark_shutdown();
        assert_eq!(h.readyz().status, StatusCode::SERVICE_UNAVAILABLE);
    }
}

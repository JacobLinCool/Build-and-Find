use crate::http::{Response, StatusCode};
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct Health {
    inner: Arc<RwLock<HealthState>>,
}

#[derive(Default)]
struct HealthState {
    ready: bool,
    live: bool,
}

impl Health {
    pub fn new() -> Self {
        let h = Self::default();
        {
            let mut s = h.inner.write();
            s.live = true;
            s.ready = false;
        }
        h
    }

    pub fn mark_ready(&self) {
        self.inner.write().ready = true;
    }

    pub fn mark_unready(&self) {
        self.inner.write().ready = false;
    }

    pub fn is_ready(&self) -> bool {
        self.inner.read().ready
    }

    pub fn is_live(&self) -> bool {
        self.inner.read().live
    }

    pub fn liveness_response(&self) -> Response {
        if self.is_live() {
            let mut r = Response::new(StatusCode::OK);
            r.body = b"OK".to_vec();
            r.headers.insert("Content-Type", "text/plain");
            r
        } else {
            Response::new(StatusCode::SERVICE_UNAVAILABLE)
        }
    }

    pub fn readiness_response(&self) -> Response {
        if self.is_ready() {
            let mut r = Response::new(StatusCode::OK);
            r.body = b"OK".to_vec();
            r.headers.insert("Content-Type", "text/plain");
            r
        } else {
            let mut r = Response::new(StatusCode::SERVICE_UNAVAILABLE);
            r.body = b"not ready".to_vec();
            r
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_default_false() {
        let h = Health::new();
        assert!(!h.is_ready());
        h.mark_ready();
        assert!(h.is_ready());
    }
}

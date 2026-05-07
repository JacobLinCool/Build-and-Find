use crate::http::{Response, StatusCode};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub struct Health {
    ready: AtomicBool,
}

impl Health {
    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    pub fn healthz(&self) -> Response {
        Response::new(StatusCode::OK, "ok")
    }

    pub fn readyz(&self) -> Response {
        if self.ready.load(Ordering::SeqCst) {
            Response::new(StatusCode::OK, "ready")
        } else {
            Response::new(StatusCode::FORBIDDEN, "not ready")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_changes() {
        let h = Health::default();
        assert_eq!(h.readyz().status.0, 403);
        h.set_ready(true);
        assert_eq!(h.readyz().status.0, 200);
    }
}

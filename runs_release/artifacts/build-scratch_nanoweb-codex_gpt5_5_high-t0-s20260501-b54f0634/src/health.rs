use crate::http::Response;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub fn healthz() -> Response {
    Response::text(200, "ok")
}

pub fn readyz(ready: &Arc<AtomicBool>) -> Response {
    if ready.load(Ordering::SeqCst) {
        Response::text(200, "ready")
    } else {
        Response::text(503, "not ready")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_tracks_flag() {
        let flag = Arc::new(AtomicBool::new(false));
        assert_eq!(readyz(&flag).status, 503);
        flag.store(true, Ordering::SeqCst);
        assert_eq!(readyz(&flag).status, 200);
    }
}

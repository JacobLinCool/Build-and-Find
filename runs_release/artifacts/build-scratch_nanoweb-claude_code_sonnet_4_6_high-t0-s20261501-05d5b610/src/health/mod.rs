use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::NanowebError;
use crate::http::{Request, Response};

/// Tracks server liveness and readiness state.
#[derive(Clone)]
pub struct HealthState {
    live: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            live: Arc::new(AtomicBool::new(true)),
            ready: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::Relaxed);
    }

    pub fn set_live(&self, live: bool) {
        self.live.store(live, Ordering::Relaxed);
    }

    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::Relaxed)
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }
}

impl Default for HealthState {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn healthz_handler(
    state: Arc<HealthState>,
    _req: Request,
) -> Result<Response, NanowebError> {
    if state.is_live() {
        Ok(Response::ok().with_text("OK"))
    } else {
        Ok(Response::new(503).with_text("Service Unavailable"))
    }
}

pub async fn readyz_handler(
    state: Arc<HealthState>,
    _req: Request,
) -> Result<Response, NanowebError> {
    if state.is_ready() {
        Ok(Response::ok().with_text("OK"))
    } else {
        Ok(Response::new(503).with_text("Not Ready"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Headers, Method, Version};

    fn make_req() -> Request {
        Request::new(
            Method::Get,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        )
    }

    #[tokio::test]
    async fn test_healthz_when_live() {
        let state = Arc::new(HealthState::new());
        let resp = healthz_handler(state, make_req()).await.unwrap();
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn test_healthz_when_dead() {
        let state = Arc::new(HealthState::new());
        state.set_live(false);
        let resp = healthz_handler(state, make_req()).await.unwrap();
        assert_eq!(resp.status, 503);
    }

    #[tokio::test]
    async fn test_readyz_before_ready() {
        let state = Arc::new(HealthState::new());
        let resp = readyz_handler(state, make_req()).await.unwrap();
        assert_eq!(resp.status, 503);
    }

    #[tokio::test]
    async fn test_readyz_when_ready() {
        let state = Arc::new(HealthState::new());
        state.set_ready(true);
        let resp = readyz_handler(state, make_req()).await.unwrap();
        assert_eq!(resp.status, 200);
    }
}

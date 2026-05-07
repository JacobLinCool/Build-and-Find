use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use crate::middleware::BoxFuture;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct HealthState {
    pub live: AtomicBool,
    pub ready: AtomicBool,
}

impl HealthState {
    pub fn new() -> Self {
        HealthState {
            live: AtomicBool::new(true),
            ready: AtomicBool::new(false),
        }
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    pub fn set_live(&self, live: bool) {
        self.live.store(live, Ordering::SeqCst);
    }
}

impl Default for HealthState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn liveness_handler(
    state: Arc<HealthState>,
) -> impl Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync + 'static {
    move |_req: Request| {
        let state = state.clone();
        Box::pin(async move {
            if state.live.load(Ordering::SeqCst) {
                let mut resp = Response::text("OK");
                resp.set_header("Content-Type", "text/plain");
                Ok(resp)
            } else {
                Ok(Response::with_body(503, b"Service Unavailable".to_vec()))
            }
        })
    }
}

pub fn readiness_handler(
    state: Arc<HealthState>,
) -> impl Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync + 'static {
    move |_req: Request| {
        let state = state.clone();
        Box::pin(async move {
            if state.ready.load(Ordering::SeqCst) {
                let mut resp = Response::text("OK");
                resp.set_header("Content-Type", "text/plain");
                Ok(resp)
            } else {
                Ok(Response::with_body(503, b"Not Ready".to_vec()))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{HttpVersion, Method};
    use crate::middleware::FnHandler;
    use std::collections::HashMap;

    fn make_req() -> Request {
        Request {
            method: Method::Get,
            target: "/healthz".into(),
            path: "/healthz".into(),
            query: None,
            version: HttpVersion::Http11,
            headers: vec![],
            body: vec![],
            params: HashMap::new(),
            remote_addr: "127.0.0.1".into(),
            request_id: "test".into(),
        }
    }

    #[tokio::test]
    async fn test_liveness_up() {
        let state = Arc::new(HealthState::new());
        let handler = liveness_handler(state.clone());
        let resp = handler(make_req()).await.unwrap();
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn test_readiness_not_ready() {
        let state = Arc::new(HealthState::new());
        let handler = readiness_handler(state.clone());
        let resp = handler(make_req()).await.unwrap();
        assert_eq!(resp.status, 503);
    }

    #[tokio::test]
    async fn test_readiness_ready() {
        let state = Arc::new(HealthState::new());
        state.set_ready(true);
        let handler = readiness_handler(state.clone());
        let resp = handler(make_req()).await.unwrap();
        assert_eq!(resp.status, 200);
    }
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::http::{Request, Response, StatusCode};

#[derive(Clone)]
pub struct Health {
    ready: Arc<AtomicBool>,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Health {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    pub fn healthz(&self) -> impl crate::router::Handler {
        |_req: Request| async { Ok(Response::text(StatusCode::OK, "ok")) }
    }

    pub fn readyz(&self) -> impl crate::router::Handler {
        let ready = self.ready.clone();
        move |_req: Request| {
            let ready = ready.clone();
            async move {
                if ready.load(Ordering::SeqCst) {
                    Ok(Response::text(StatusCode::OK, "ready"))
                } else {
                    Ok(Response::text(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "not ready",
                    ))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;
    use crate::router::Handler;

    #[tokio::test]
    async fn ready_changes_status() {
        let health = Health::new();
        let res = health
            .readyz()
            .call(Request::new(Method::Get, "/readyz"))
            .await
            .unwrap();
        assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR);
        health.set_ready(true);
        let res = health
            .readyz()
            .call(Request::new(Method::Get, "/readyz"))
            .await
            .unwrap();
        assert_eq!(res.status, StatusCode::OK);
    }
}

use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub struct RateLimit {
    state: Mutex<HashMap<String, (f64, Instant)>>,
    capacity: f64,
    refill_per_sec: f64,
}

impl RateLimit {
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            capacity,
            refill_per_sec,
        }
    }
}

#[async_trait]
impl Middleware for RateLimit {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        let ip = req
            .remote_addr
            .map(|a| a.ip().to_string())
            .unwrap_or_else(|| "unknown".into());
        let mut state = self.state.lock().unwrap();
        let (tokens, last) = state.entry(ip).or_insert((self.capacity, Instant::now()));
        let elapsed = last.elapsed().as_secs_f64();
        *tokens = (*tokens + elapsed * self.refill_per_sec).min(self.capacity);
        *last = Instant::now();
        if *tokens < 1.0 {
            return Some(Response::text(429, "rate limited"));
        }
        *tokens -= 1.0;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn limits() {
        let rl = RateLimit::new(1.0, 0.0);
        let mut req = Request {
            method: crate::http::Method::Get,
            target: "/".into(),
            version: crate::http::Version::Http11,
            headers: Default::default(),
            body: vec![],
            path_params: Default::default(),
            query_params: Default::default(),
            remote_addr: None,
            request_id: "1".into(),
        };
        assert!(rl.before(&mut req).await.is_none());
        assert!(rl.before(&mut req).await.is_some());
    }
}

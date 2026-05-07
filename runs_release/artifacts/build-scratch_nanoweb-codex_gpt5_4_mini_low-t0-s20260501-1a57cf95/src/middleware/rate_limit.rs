use super::Middleware;
use crate::http::types::{Request, Response};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub struct RateLimit {
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
    rate: f64,
    burst: f64,
}
impl Default for RateLimit {
    fn default() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            rate: 5.0,
            burst: 10.0,
        }
    }
}
impl Middleware for RateLimit {
    fn before(&self, req: &mut Request) -> Result<Option<Response>, crate::NanowebError> {
        let key = req
            .peer_addr
            .map(|a| a.ip().to_string())
            .unwrap_or_else(|| "unknown".into());
        let mut b = self.buckets.lock().unwrap();
        let (tokens, last) = b.entry(key).or_insert((self.burst, Instant::now()));
        let now = Instant::now();
        let added = now.duration_since(*last).as_secs_f64() * self.rate;
        *tokens = (*tokens + added).min(self.burst);
        *last = now;
        if *tokens < 1.0 {
            return Ok(Some(Response::text(429, "rate limit exceeded")));
        }
        *tokens -= 1.0;
        Ok(None)
    }
}

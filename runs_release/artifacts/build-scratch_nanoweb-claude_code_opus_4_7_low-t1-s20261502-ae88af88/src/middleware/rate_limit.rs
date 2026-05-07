use super::Middleware;
use crate::http::{Request, Response, StatusCode};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::Instant;

/// Token-bucket rate limiter keyed by client IP.
pub struct RateLimit {
    capacity: f64,
    refill_per_sec: f64,
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
}

impl RateLimit {
    pub fn new(capacity: u32, refill_per_sec: f64) -> Self {
        Self {
            capacity: capacity as f64,
            refill_per_sec,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn allow(&self, key: &str) -> bool {
        let mut g = self.buckets.lock();
        let now = Instant::now();
        let entry = g.entry(key.to_string()).or_insert((self.capacity, now));
        let elapsed = now.duration_since(entry.1).as_secs_f64();
        entry.0 = (entry.0 + elapsed * self.refill_per_sec).min(self.capacity);
        entry.1 = now;
        if entry.0 >= 1.0 {
            entry.0 -= 1.0;
            true
        } else {
            false
        }
    }
}

#[async_trait]
impl Middleware for RateLimit {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        let key = req
            .remote_addr
            .map(|a| a.ip().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        if self.allow(&key) {
            None
        } else {
            let mut r = Response::new(StatusCode::TOO_MANY_REQUESTS);
            r.headers.insert("Content-Type", "text/plain");
            r.body = b"rate limit exceeded".to_vec();
            Some(r)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_bucket() {
        let rl = RateLimit::new(2, 0.0);
        assert!(rl.allow("a"));
        assert!(rl.allow("a"));
        assert!(!rl.allow("a"));
    }
}

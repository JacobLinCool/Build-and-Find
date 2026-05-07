//! Token-bucket rate limiting per client IP.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::error::Result;
use crate::http::{Request, Response, StatusCode};

use super::{BoxFuture, Middleware, Next};

#[derive(Clone, Copy)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct RateLimitMiddleware {
    capacity: f64,
    refill_per_sec: f64,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimitMiddleware {
    pub fn new(capacity: u32, refill_per_sec: f64) -> Self {
        Self {
            capacity: capacity as f64,
            refill_per_sec,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn allow(&self, ip: &str) -> bool {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().expect("rate limit lock");
        let bucket = buckets.entry(ip.to_string()).or_insert(Bucket {
            tokens: self.capacity,
            last: now,
        });
        let elapsed = now.duration_since(bucket.last).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        bucket.last = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

impl Middleware for RateLimitMiddleware {
    fn handle<'a>(
        &'a self,
        req: &'a mut Request,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            let ip = req.remote_ip.clone();
            if !self.allow(&ip) {
                return Ok(Response::new(StatusCode::TOO_MANY_REQUESTS).text("rate limit exceeded"));
            }
            next.run(req).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_is_enforced() {
        let rl = RateLimitMiddleware::new(2, 0.0);
        assert!(rl.allow("1.1.1.1"));
        assert!(rl.allow("1.1.1.1"));
        assert!(!rl.allow("1.1.1.1"));
        // Different IP has its own bucket.
        assert!(rl.allow("2.2.2.2"));
    }
}

use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

pub struct RateLimitMiddleware {
    capacity: f64,
    refill_rate: f64, // tokens per second
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimitMiddleware {
    pub fn new(capacity: u32, refill_rate: f64) -> Self {
        Self {
            capacity: capacity as f64,
            refill_rate,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn check(&self, key: &str) -> bool {
        let mut buckets = self.buckets.lock().unwrap();
        let now = Instant::now();
        let bucket = buckets.entry(key.to_string()).or_insert(Bucket {
            tokens: self.capacity,
            last_refill: now,
        });

        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.refill_rate).min(self.capacity);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[async_trait]
impl Middleware for RateLimitMiddleware {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        let key = req.addr.ip().to_string();
        if !self.check(&key) {
            Some(Response::text(429, "Too Many Requests"))
        } else {
            None
        }
    }
}

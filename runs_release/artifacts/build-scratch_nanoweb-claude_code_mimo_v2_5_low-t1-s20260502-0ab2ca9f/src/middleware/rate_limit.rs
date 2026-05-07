use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::http::{Request, Response};
use crate::middleware::Middleware;

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    capacity: f64,
    refill_rate: f64, // tokens per second
}

impl TokenBucket {
    fn new(capacity: f64, refill_rate: f64) -> Self {
        TokenBucket {
            tokens: capacity,
            last_refill: Instant::now(),
            capacity,
            refill_rate,
        }
    }

    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn retry_after(&self) -> u64 {
        if self.tokens >= 1.0 {
            0
        } else {
            ((1.0 - self.tokens) / self.refill_rate).ceil() as u64
        }
    }
}

pub struct RateLimitMiddleware {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    capacity: f64,
    refill_rate: f64,
}

impl RateLimitMiddleware {
    pub fn new(requests_per_second: f64) -> Self {
        RateLimitMiddleware {
            buckets: Mutex::new(HashMap::new()),
            capacity: requests_per_second,
            refill_rate: requests_per_second,
        }
    }
}

impl Middleware for RateLimitMiddleware {
    fn before(&self, req: &mut Request) -> Option<Response> {
        let ip = req.client_ip().to_string();
        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets
            .entry(ip.clone())
            .or_insert_with(|| TokenBucket::new(self.capacity, self.refill_rate));

        if bucket.try_consume() {
            None
        } else {
            let retry = bucket.retry_after();
            let mut res = Response::new();
            res.status = crate::http::StatusCode::new(429);
            res.headers.insert("retry-after".into(), retry.to_string());
            res.set_text("Too Many Requests");
            Some(res)
        }
    }
}

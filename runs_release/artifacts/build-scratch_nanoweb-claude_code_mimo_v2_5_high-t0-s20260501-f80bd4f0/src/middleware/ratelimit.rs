use crate::http::types::{Request, Response, StatusCode};
use crate::middleware::Middleware;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    rate: f64,
    burst: f64,
}

impl TokenBucket {
    fn new(rate: f64, burst: f64) -> Self {
        Self {
            tokens: burst,
            last_refill: Instant::now(),
            rate,
            burst,
        }
    }

    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate).min(self.burst);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub struct RateLimiter {
    buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
    rate: f64,
    burst: f64,
}

impl RateLimiter {
    pub fn new(requests_per_second: u32, burst: u32) -> Self {
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            rate: requests_per_second as f64,
            burst: burst as f64,
        }
    }
}

impl Middleware for RateLimiter {
    fn before(&self, req: &mut Request) -> Option<Response> {
        let ip = req
            .header("x-forwarded-for")
            .or_else(|| req.header("x-real-ip"))
            .unwrap_or("unknown")
            .to_string();

        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets
            .entry(ip)
            .or_insert_with(|| TokenBucket::new(self.rate, self.burst));

        if bucket.try_consume() {
            None
        } else {
            Some(Response::new(StatusCode(429)).with_header("retry-after", "1"))
        }
    }

    fn name(&self) -> &str {
        "rate-limit"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_allows_normal_traffic() {
        let limiter = RateLimiter::new(10, 10);
        let mut req = Request {
            method: crate::http::types::Method::Get,
            path: "/".into(),
            query: String::new(),
            version: "HTTP/1.1".into(),
            headers: crate::http::types::Headers::new(),
            body: Vec::new(),
            params: Default::default(),
            request_id: "test".into(),
        };
        // First request should pass
        assert!(limiter.before(&mut req).is_none());
    }
}

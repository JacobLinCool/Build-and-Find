use super::Middleware;
use crate::http::types::{Request, Response};
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
    pub fn new(requests_per_second: f64) -> Self {
        Self {
            capacity: requests_per_second,
            refill_rate: requests_per_second,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn check_rate(&self, key: &str) -> bool {
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

impl Middleware for RateLimitMiddleware {
    fn name(&self) -> &str {
        "rate_limit"
    }

    fn before(&self, req: &mut Request) -> Option<Response> {
        let key = req.remote_addr.as_deref().unwrap_or("unknown");
        let ip = key.split(':').next().unwrap_or(key);

        if !self.check_rate(ip) {
            let mut resp = Response::text(429, "Too Many Requests");
            resp.set_header("retry-after", "1");
            Some(resp)
        } else {
            None
        }
    }
}

unsafe impl Send for RateLimitMiddleware {}
unsafe impl Sync for RateLimitMiddleware {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_req() -> Request {
        Request {
            method: crate::http::types::Method::Get,
            target: "/".to_string(),
            path: "/".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            params: HashMap::new(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: Some("192.168.1.1:12345".to_string()),
        }
    }

    #[test]
    fn test_rate_limit_allows_initial_requests() {
        let mw = RateLimitMiddleware::new(10.0);
        let mut req = make_req();
        assert!(mw.before(&mut req).is_none());
    }

    #[test]
    fn test_rate_limit_blocks_after_capacity() {
        let mw = RateLimitMiddleware::new(2.0);
        let mut req1 = make_req();
        let mut req2 = make_req();
        let mut req3 = make_req();
        assert!(mw.before(&mut req1).is_none());
        assert!(mw.before(&mut req2).is_none());
        assert!(mw.before(&mut req3).is_some()); // blocked
    }
}

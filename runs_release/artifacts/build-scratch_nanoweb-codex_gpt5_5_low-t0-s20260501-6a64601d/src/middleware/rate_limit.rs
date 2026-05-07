use super::Middleware;
use crate::http::{Request, Response, StatusCode};
use async_trait::async_trait;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Instant;

struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct RateLimit {
    capacity: f64,
    refill_per_s: f64,
    buckets: Mutex<HashMap<IpAddr, Bucket>>,
}

impl RateLimit {
    pub fn new(capacity: u32, refill_per_s: f64) -> Self {
        Self {
            capacity: capacity as f64,
            refill_per_s,
            buckets: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl Middleware for RateLimit {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        let ip = req
            .peer_addr
            .map(|a| a.ip())
            .unwrap_or_else(|| "127.0.0.1".parse().unwrap());
        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets.entry(ip).or_insert(Bucket {
            tokens: self.capacity,
            last: Instant::now(),
        });
        let now = Instant::now();
        bucket.tokens = (bucket.tokens
            + now.duration_since(bucket.last).as_secs_f64() * self.refill_per_s)
            .min(self.capacity);
        bucket.last = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            None
        } else {
            Some(Response::text(
                StatusCode::PayloadTooLarge,
                "rate limit exceeded",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn construct() {
        let _ = super::RateLimit::new(1, 1.0);
    }
}

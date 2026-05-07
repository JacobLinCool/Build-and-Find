use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;
use std::time::Instant;

use async_trait::async_trait;

use crate::error::Result;
use crate::http::{Request, Response, StatusCode};
use crate::middleware::Middleware;

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct RateLimit {
    capacity: f64,
    refill_per_second: f64,
    buckets: Mutex<HashMap<IpAddr, Bucket>>,
}

impl RateLimit {
    pub fn new(capacity: u32, refill_per_second: f64) -> Self {
        Self {
            capacity: capacity.max(1) as f64,
            refill_per_second: refill_per_second.max(0.1),
            buckets: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl Middleware for RateLimit {
    async fn before(&self, request: &mut Request) -> Result<Option<Response>> {
        let ip = request
            .peer_addr
            .map(|addr| addr.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let mut buckets = self.buckets.lock().expect("rate-limit mutex poisoned");
        let bucket = buckets.entry(ip).or_insert_with(|| Bucket {
            tokens: self.capacity,
            last: Instant::now(),
        });
        let elapsed = bucket.last.elapsed().as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.refill_per_second).min(self.capacity);
        bucket.last = Instant::now();
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(None)
        } else {
            Ok(Some(Response::text(
                StatusCode::TOO_MANY_REQUESTS,
                "too many requests",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;

    #[tokio::test]
    async fn limits_after_capacity() {
        let limiter = RateLimit::new(1, 0.1);
        let mut req = Request::new(Method::Get, "/");
        assert!(limiter.before(&mut req).await.unwrap().is_none());
        assert!(limiter.before(&mut req).await.unwrap().is_some());
    }
}

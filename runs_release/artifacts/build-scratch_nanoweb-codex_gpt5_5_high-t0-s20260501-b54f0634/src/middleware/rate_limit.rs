use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct RateLimit {
    capacity: f64,
    refill_per_sec: f64,
    buckets: Mutex<HashMap<IpAddr, Bucket>>,
}

impl RateLimit {
    pub fn new(capacity: u32, refill_per_sec: f64) -> Self {
        Self {
            capacity: capacity as f64,
            refill_per_sec,
            buckets: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl Middleware for RateLimit {
    async fn before(&self, request: &mut Request) -> Option<Response> {
        let ip = request.remote_addr.map(|a| a.ip())?;
        let mut buckets = self.buckets.lock().expect("rate limit mutex poisoned");
        let now = Instant::now();
        let bucket = buckets.entry(ip).or_insert(Bucket {
            tokens: self.capacity,
            last: now,
        });
        let elapsed = now.duration_since(bucket.last).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        bucket.last = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            None
        } else {
            let mut response = Response::text(429, "rate limit exceeded");
            response.set_header("Retry-After", Duration::from_secs(1).as_secs().to_string());
            Some(response)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddr};

    #[tokio::test]
    async fn limits_by_ip() {
        let limiter = RateLimit::new(1, 0.0);
        let mut req = Request::new(crate::http::Method::Get, "/");
        req.remote_addr = Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1));
        assert!(limiter.before(&mut req.clone()).await.is_none());
        assert_eq!(limiter.before(&mut req).await.unwrap().status, 429);
    }
}

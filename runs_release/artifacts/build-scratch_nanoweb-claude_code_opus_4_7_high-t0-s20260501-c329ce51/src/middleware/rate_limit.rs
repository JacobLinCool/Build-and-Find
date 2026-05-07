use super::Middleware;
use crate::http::{Request, Response, StatusCode};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, Debug)]
pub struct TokenBucketConfig {
    pub capacity: f64,
    pub refill_per_sec: f64,
}

#[derive(Default)]
struct Buckets {
    map: HashMap<IpAddr, BucketState>,
}

#[derive(Clone, Copy)]
struct BucketState {
    tokens: f64,
    last: Instant,
}

pub struct RateLimit {
    cfg: TokenBucketConfig,
    state: Arc<Mutex<Buckets>>,
}

impl RateLimit {
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            cfg: TokenBucketConfig {
                capacity,
                refill_per_sec,
            },
            state: Arc::new(Mutex::new(Buckets::default())),
        }
    }

    fn check(&self, ip: IpAddr) -> bool {
        let mut b = self.state.lock();
        let now = Instant::now();
        let entry = b.map.entry(ip).or_insert(BucketState {
            tokens: self.cfg.capacity,
            last: now,
        });
        let elapsed = now.duration_since(entry.last).as_secs_f64();
        entry.tokens = (entry.tokens + elapsed * self.cfg.refill_per_sec).min(self.cfg.capacity);
        entry.last = now;
        if entry.tokens >= 1.0 {
            entry.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[async_trait]
impl Middleware for RateLimit {
    fn name(&self) -> &'static str {
        "rate_limit"
    }

    async fn before(&self, req: &mut Request) -> Option<Response> {
        let ip = match req.remote_addr {
            Some(addr) => addr.ip(),
            None => return None,
        };
        if self.check(ip) {
            None
        } else {
            let mut r = Response::new(StatusCode::TOO_MANY_REQUESTS);
            r.headers.insert("Retry-After", "1");
            r.body = b"rate limited".to_vec();
            Some(r)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, Version};

    #[tokio::test]
    async fn limits_after_burst() {
        let rl = RateLimit::new(2.0, 0.0);
        let addr: std::net::SocketAddr = "127.0.0.1:1234".parse().unwrap();
        let mut allow = 0;
        for _ in 0..5 {
            let mut req = Request::new(Method::Get, "/".into(), Version::Http11);
            req.remote_addr = Some(addr);
            if rl.before(&mut req).await.is_none() {
                allow += 1;
            }
        }
        assert_eq!(allow, 2);
    }
}

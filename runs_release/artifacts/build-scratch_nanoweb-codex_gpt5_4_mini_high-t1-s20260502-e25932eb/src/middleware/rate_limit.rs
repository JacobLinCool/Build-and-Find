use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::error::Result;
use crate::http::{Request, Response, StatusCode};

use super::{Middleware, Next};

#[derive(Clone)]
pub struct RateLimitConfig {
    pub permits_per_second: f64,
    pub burst: f64,
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

pub struct RateLimit {
    config: RateLimitConfig,
    buckets: Arc<Mutex<HashMap<IpAddr, Bucket>>>,
}

impl RateLimit {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Middleware for RateLimit {
    async fn handle(&self, request: Request, next: Next<'_>) -> Result<Response> {
        let Some(addr) = request.remote_addr.map(|addr| addr.ip()) else {
            return next.run(request).await;
        };
        let allowed = {
            let mut buckets = self.buckets.lock();
            let bucket = buckets.entry(addr).or_insert_with(|| Bucket {
                tokens: self.config.burst,
                last_refill: Instant::now(),
            });
            let now = Instant::now();
            let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
            bucket.tokens =
                (bucket.tokens + elapsed * self.config.permits_per_second).min(self.config.burst);
            bucket.last_refill = now;
            if bucket.tokens < 1.0 {
                false
            } else {
                bucket.tokens -= 1.0;
                true
            }
        };
        if !allowed {
            return Ok(Response::text(
                StatusCode::TooManyRequests,
                "rate limit exceeded",
            ));
        }
        next.run(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HeaderMap, Method, Version};
    use crate::router::Handler;
    use std::net::SocketAddr;
    use std::sync::Arc;

    struct OkHandler;

    #[async_trait]
    impl Handler for OkHandler {
        async fn handle(&self, _request: Request) -> Result<Response> {
            Ok(Response::text(StatusCode::Ok, "ok"))
        }
    }

    #[tokio::test]
    async fn rejects_when_bucket_empty() {
        let limiter = RateLimit::new(RateLimitConfig {
            permits_per_second: 0.0,
            burst: 0.0,
        });
        let request = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: Version::Http11,
            headers: HeaderMap::new(),
            body: vec![],
            params: Default::default(),
            remote_addr: Some("127.0.0.1:1234".parse::<SocketAddr>().unwrap()),
            request_id: "req".into(),
            session: None,
        };
        let response =
            crate::middleware::execute(&[Arc::new(limiter)], Arc::new(OkHandler), request)
                .await
                .unwrap();
        assert_eq!(response.status, StatusCode::TooManyRequests);
    }
}

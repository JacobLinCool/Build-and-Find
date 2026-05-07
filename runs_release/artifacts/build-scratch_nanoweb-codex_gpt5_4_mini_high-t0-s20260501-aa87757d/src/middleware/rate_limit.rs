use crate::{
    app::AppState,
    error::Result,
    http::types::{Request, Response, StatusCode},
    middleware::{Middleware, Next},
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Instant,
};

pub struct RateLimitMiddleware {
    tokens_per_second: f64,
    burst: u32,
    buckets: Arc<Mutex<HashMap<IpAddr, Bucket>>>,
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimitMiddleware {
    pub fn new(tokens_per_second: f64, burst: u32) -> Self {
        Self {
            tokens_per_second,
            burst,
            buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Middleware for RateLimitMiddleware {
    async fn handle(&self, req: Request, ctx: Arc<AppState>, next: Next<'_>) -> Result<Response> {
        let ip = req
            .remote_addr
            .map(|addr| addr.ip())
            .unwrap_or(std::net::IpAddr::from([127, 0, 0, 1]));
        let allowed = {
            let mut buckets = self.buckets.lock().unwrap();
            let bucket = buckets.entry(ip).or_insert_with(|| Bucket {
                tokens: self.burst as f64,
                last_refill: Instant::now(),
            });
            let elapsed = bucket.last_refill.elapsed().as_secs_f64();
            bucket.tokens =
                (bucket.tokens + elapsed * self.tokens_per_second).min(self.burst as f64);
            bucket.last_refill = Instant::now();
            if bucket.tokens >= 1.0 {
                bucket.tokens -= 1.0;
                true
            } else {
                false
            }
        };
        if !allowed {
            return Ok(Response::text(
                StatusCode::TooManyRequests,
                "rate limit exceeded",
            ));
        }
        let _ = ctx;
        next.run(req).await
    }
}

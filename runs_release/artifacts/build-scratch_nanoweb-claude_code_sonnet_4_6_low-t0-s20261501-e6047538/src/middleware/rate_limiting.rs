use super::{Middleware, Next};
use crate::error::NanowebError;
use crate::http::types::{Request, Response, Status};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    rate: f64, // tokens per second
    capacity: f64,
}

impl TokenBucket {
    fn new(rate: f64, capacity: f64) -> Self {
        TokenBucket {
            tokens: capacity,
            last_refill: Instant::now(),
            rate,
            capacity,
        }
    }

    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate).min(self.capacity);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub struct RateLimitMiddleware {
    rate: f64,
    burst: f64,
    buckets: Mutex<HashMap<String, TokenBucket>>,
}

impl RateLimitMiddleware {
    pub fn new(requests_per_second: f64, burst: f64) -> Self {
        RateLimitMiddleware {
            rate: requests_per_second,
            burst,
            buckets: Mutex::new(HashMap::new()),
        }
    }
}

fn extract_client_ip(req: &Request) -> String {
    req.header("x-forwarded-for")
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[async_trait]
impl Middleware for RateLimitMiddleware {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
        let ip = extract_client_ip(&req);

        let allowed = {
            let mut buckets = self.buckets.lock().unwrap();
            let bucket = buckets
                .entry(ip)
                .or_insert_with(|| TokenBucket::new(self.rate, self.burst));
            bucket.try_consume()
        };

        if allowed {
            next.run(req).await
        } else {
            Ok(Response::new(Status::TooManyRequests)
                .with_text("Rate limit exceeded")
                .set_header("retry-after", "1"))
        }
    }
}

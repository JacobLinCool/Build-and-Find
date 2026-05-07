use crate::error::NanowebError;
use crate::http::types::{Request, Response, StatusCode};
use crate::middleware::{BoxFuture, Middleware, Next};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
}

impl TokenBucket {
    fn new(max_tokens: f64, refill_rate: f64) -> Self {
        TokenBucket {
            tokens: max_tokens,
            last_refill: Instant::now(),
            max_tokens,
            refill_rate,
        }
    }

    fn try_consume(&mut self, tokens: f64) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;

        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }
}

pub struct RateLimiter {
    buckets: Arc<Mutex<HashMap<IpAddr, TokenBucket>>>,
    max_tokens: f64,
    refill_rate: f64,
    cleanup_interval: Duration,
}

impl RateLimiter {
    pub fn new(max_requests_per_second: f64, burst: f64) -> Self {
        RateLimiter {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            max_tokens: burst,
            refill_rate: max_requests_per_second,
            cleanup_interval: Duration::from_secs(60),
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        RateLimiter::new(10.0, 20.0)
    }
}

impl Middleware for RateLimiter {
    fn handle<'a>(&'a self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
        let buckets = self.buckets.clone();
        let max_tokens = self.max_tokens;
        let refill_rate = self.refill_rate;
        let ip = req.remote_addr.ip();

        Box::pin(async move {
            let allowed = {
                let mut map = buckets.lock().await;
                let bucket = map
                    .entry(ip)
                    .or_insert_with(|| TokenBucket::new(max_tokens, refill_rate));
                bucket.try_consume(1.0)
            };

            if !allowed {
                let mut resp = Response::new(StatusCode::TOO_MANY_REQUESTS);
                resp.headers.set("retry-after", "1");
                resp.body = b"Too Many Requests".to_vec();
                return Ok(resp);
            }

            next.run(req).await
        })
    }
}

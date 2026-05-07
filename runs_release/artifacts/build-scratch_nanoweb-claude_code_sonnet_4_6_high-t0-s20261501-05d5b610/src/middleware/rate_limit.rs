use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::NanowebError;
use crate::http::{Request, Response};
use crate::middleware::{Middleware, Next};

#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: Instant::now(),
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

pub struct RateLimitMiddleware {
    buckets: Arc<Mutex<HashMap<IpAddr, TokenBucket>>>,
    max_tokens: f64,
    refill_rate: f64,
    cleanup_interval: Duration,
    last_cleanup: Mutex<Instant>,
}

impl RateLimitMiddleware {
    /// Create rate limiter: max_requests burst, refill_rate tokens/second.
    pub fn new(max_requests: u32, refill_rate: f64) -> Self {
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            max_tokens: max_requests as f64,
            refill_rate,
            cleanup_interval: Duration::from_secs(60),
            last_cleanup: Mutex::new(Instant::now()),
        }
    }

    fn check_and_consume(&self, ip: IpAddr) -> bool {
        let mut buckets = self.buckets.lock().unwrap();

        // Periodic cleanup of old entries
        {
            let mut last = self.last_cleanup.lock().unwrap();
            if last.elapsed() > self.cleanup_interval {
                buckets.retain(|_, b| {
                    let elapsed = b.last_refill.elapsed().as_secs_f64();
                    let projected = b.tokens + elapsed * b.refill_rate;
                    projected < b.max_tokens // Keep if not fully refilled (was recently used)
                });
                *last = Instant::now();
            }
        }

        let bucket = buckets
            .entry(ip)
            .or_insert_with(|| TokenBucket::new(self.max_tokens, self.refill_rate));

        bucket.try_consume(1.0)
    }
}

#[async_trait::async_trait]
impl Middleware for RateLimitMiddleware {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
        let ip = req
            .client_ip()
            .unwrap_or_else(|| "0.0.0.0".parse().unwrap());

        if !self.check_and_consume(ip) {
            let mut resp = Response::new(429);
            resp.headers.insert("Retry-After", "1");
            resp.body = b"Too Many Requests".to_vec();
            return Ok(resp);
        }

        next.run(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Headers, Method, Version};
    use crate::middleware::MiddlewareChain;
    use std::net::SocketAddr;

    fn make_req(ip: &str) -> Request {
        let addr: SocketAddr = format!("{}:1234", ip).parse().unwrap();
        Request::new(
            Method::Get,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            Some(addr),
            "id".to_string(),
        )
    }

    #[tokio::test]
    async fn test_rate_limit_allows_within_limit() {
        let mw = Arc::new(RateLimitMiddleware::new(5, 1.0));
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok()) })
        });

        for _ in 0..5 {
            let chain = MiddlewareChain::new(vec![mw.clone()], handler.clone());
            let resp = chain.run(make_req("127.0.0.1")).await.unwrap();
            assert_eq!(resp.status, 200);
        }
    }

    #[tokio::test]
    async fn test_rate_limit_blocks_over_limit() {
        let mw = Arc::new(RateLimitMiddleware::new(2, 0.1));
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok()) })
        });

        // Exhaust tokens
        for _ in 0..2 {
            let chain = MiddlewareChain::new(vec![mw.clone()], handler.clone());
            chain.run(make_req("10.0.0.1")).await.unwrap();
        }

        // Next request should be rate limited
        let chain = MiddlewareChain::new(vec![mw.clone()], handler.clone());
        let resp = chain.run(make_req("10.0.0.1")).await.unwrap();
        assert_eq!(resp.status, 429);
    }

    #[tokio::test]
    async fn test_rate_limit_per_ip() {
        let mw = Arc::new(RateLimitMiddleware::new(1, 0.1));
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok()) })
        });

        // Each IP gets its own bucket
        let chain1 = MiddlewareChain::new(vec![mw.clone()], handler.clone());
        let resp1 = chain1.run(make_req("1.1.1.1")).await.unwrap();
        assert_eq!(resp1.status, 200);

        let chain2 = MiddlewareChain::new(vec![mw.clone()], handler.clone());
        let resp2 = chain2.run(make_req("2.2.2.2")).await.unwrap();
        assert_eq!(resp2.status, 200);
    }
}

use super::Middleware;
use crate::http::{Request, Response};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Instant;

struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
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

    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
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
    buckets: Mutex<HashMap<String, TokenBucket>>,
    max_tokens: f64,
    refill_rate: f64,
}

impl RateLimitMiddleware {
    pub fn new(max_requests_per_second: f64) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            max_tokens: max_requests_per_second * 2.0,
            refill_rate: max_requests_per_second,
        }
    }

    fn get_client_ip(req: &Request) -> String {
        req.header("x-forwarded-for")
            .and_then(|v| v.split(',').next())
            .map(|s| s.trim().to_string())
            .or_else(|| req.header("x-real-ip").map(|s| s.to_string()))
            .or_else(|| req.remote_addr.map(|a| a.ip().to_string()))
            .unwrap_or_else(|| "unknown".to_string())
    }
}

impl Middleware for RateLimitMiddleware {
    fn before(
        &self,
        req: &mut Request,
    ) -> Pin<Box<dyn Future<Output = Option<Response>> + Send + 'static>> {
        let client_ip = Self::get_client_ip(req);
        let allowed = {
            let mut buckets = self.buckets.lock().unwrap();
            let bucket = buckets
                .entry(client_ip.clone())
                .or_insert_with(|| TokenBucket::new(self.max_tokens, self.refill_rate));
            bucket.try_consume()
        };

        Box::pin(async move {
            if allowed {
                None
            } else {
                Some(
                    Response::new(429)
                        .with_header("retry-after", "1")
                        .with_body("Too Many Requests"),
                )
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limit_allows_request() {
        let mw = RateLimitMiddleware::new(10.0);
        let mut req = Request::new(crate::http::Method::Get, "/".into(), "HTTP/1.1".into());
        req.remote_addr = Some("127.0.0.1:12345".parse().unwrap());

        let result = mw.before(&mut req).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_rate_limit_blocks_excessive_requests() {
        let mw = RateLimitMiddleware::new(1.0);
        let mut req = Request::new(crate::http::Method::Get, "/".into(), "HTTP/1.1".into());
        req.remote_addr = Some("127.0.0.1:12345".parse().unwrap());

        for _ in 0..3 {
            mw.before(&mut req).await;
        }

        let result = mw.before(&mut req).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().status, 429);
    }
}

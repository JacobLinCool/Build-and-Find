use super::MiddlewareDyn;
use crate::http::{Request, Response, StatusCode};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Instant;

/// Token bucket for rate limiting.
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

/// Token bucket rate limiter per client IP.
pub struct RateLimitMiddleware {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    max_tokens: f64,
    refill_rate: f64,
}

impl RateLimitMiddleware {
    /// Create a rate limiter with `max_requests` per `per_seconds` window.
    pub fn new(max_requests: u32, per_seconds: u32) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            max_tokens: max_requests as f64,
            refill_rate: max_requests as f64 / per_seconds as f64,
        }
    }

    fn get_client_ip(req: &Request) -> String {
        req.header("x-forwarded-for")
            .and_then(|v| v.split(',').next())
            .map(|s| s.trim().to_string())
            .or_else(|| req.header("x-real-ip").map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string())
    }
}

impl MiddlewareDyn for RateLimitMiddleware {
    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Request, Response>> + Send + 'a>> {
        Box::pin(async move {
            let client_ip = Self::get_client_ip(&req);

            let mut buckets = self.buckets.lock().unwrap();
            let bucket = buckets
                .entry(client_ip)
                .or_insert_with(|| TokenBucket::new(self.max_tokens, self.refill_rate));

            if bucket.try_consume() {
                Ok(req)
            } else {
                Err(Response::text(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Rate limit exceeded",
                ))
            }
        })
    }

    fn after<'a>(
        &'a self,
        _req: &'a Request,
        resp: Response,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(async move { resp })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HeaderMap, Method};

    fn make_req() -> Request {
        Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        }
    }

    #[tokio::test]
    async fn test_rate_limit_allows_within_limit() {
        let mw = RateLimitMiddleware::new(5, 1);
        for _ in 0..5 {
            let req = make_req();
            assert!(mw.before(req).await.is_ok());
        }
    }

    #[tokio::test]
    async fn test_rate_limit_blocks_over_limit() {
        let mw = RateLimitMiddleware::new(2, 10);
        let req1 = make_req();
        assert!(mw.before(req1).await.is_ok());
        let req2 = make_req();
        assert!(mw.before(req2).await.is_ok());
        let req3 = make_req();
        assert!(mw.before(req3).await.is_err());
    }

    #[tokio::test]
    async fn test_rate_limit_different_ips() {
        let mw = RateLimitMiddleware::new(1, 10);
        let mut req1 = make_req();
        req1.headers.insert("x-forwarded-for", "1.1.1.1");
        assert!(mw.before(req1).await.is_ok());

        let mut req2 = make_req();
        req2.headers.insert("x-forwarded-for", "2.2.2.2");
        assert!(mw.before(req2).await.is_ok());
    }
}

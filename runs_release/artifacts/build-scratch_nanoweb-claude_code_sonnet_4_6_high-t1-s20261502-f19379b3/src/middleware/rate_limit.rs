use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use crate::middleware::{BoxFuture, Middleware, Next};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: f64) -> Self {
        TokenBucket {
            tokens: capacity,
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self, rate: f64, capacity: f64) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * rate).min(capacity);
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
    rate: f64,
    capacity: f64,
}

impl RateLimitMiddleware {
    pub fn new(requests_per_second: f64) -> Self {
        RateLimitMiddleware {
            buckets: Mutex::new(HashMap::new()),
            rate: requests_per_second,
            capacity: requests_per_second * 2.0,
        }
    }

    pub fn with_capacity(requests_per_second: f64, capacity: f64) -> Self {
        RateLimitMiddleware {
            buckets: Mutex::new(HashMap::new()),
            rate: requests_per_second,
            capacity,
        }
    }
}

impl Middleware for RateLimitMiddleware {
    fn call(&self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
        let client_ip = req.remote_addr.clone();
        let allowed = {
            let mut buckets = self.buckets.lock().unwrap();
            let bucket = buckets
                .entry(client_ip)
                .or_insert_with(|| TokenBucket::new(self.capacity));
            bucket.try_consume(self.rate, self.capacity)
        };

        Box::pin(async move {
            if allowed {
                next.run(req).await
            } else {
                Err(NanowebError::TooManyRequests)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{HttpVersion, Method};
    use crate::middleware::{FnHandler, Next};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn make_req() -> Request {
        Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: HttpVersion::Http11,
            headers: vec![],
            body: vec![],
            params: HashMap::new(),
            remote_addr: "10.0.0.1".into(),
            request_id: "test".into(),
        }
    }

    #[tokio::test]
    async fn test_rate_limit_allows_under_limit() {
        let handler = Arc::new(FnHandler::new(|_req: Request| async {
            Ok(Response::text("ok"))
        }));
        let rl = Arc::new(RateLimitMiddleware::new(10.0));
        let mw: Vec<Arc<dyn Middleware>> = vec![rl];
        let next = Next::new(handler, mw);
        let resp = next.run(make_req()).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limit_blocks_over_limit() {
        let handler = Arc::new(FnHandler::new(|_req: Request| async {
            Ok(Response::text("ok"))
        }));
        let rl = Arc::new(RateLimitMiddleware::with_capacity(1.0, 1.0));

        // Exhaust the bucket
        {
            let mut buckets = rl.buckets.lock().unwrap();
            buckets.insert(
                "10.0.0.1".to_string(),
                TokenBucket {
                    tokens: 0.0,
                    last_refill: Instant::now(),
                },
            );
        }

        let mw: Vec<Arc<dyn Middleware>> = vec![rl];
        let next = Next::new(handler, mw);
        let resp = next.run(make_req()).await;
        assert!(matches!(resp, Err(NanowebError::TooManyRequests)));
    }
}

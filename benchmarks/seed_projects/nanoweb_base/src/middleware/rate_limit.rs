//! Token-bucket rate limit keyed by the client IP address.
//!
//! Each remote IP gets a bucket with `capacity` tokens, refilled at
//! `refill_per_sec` tokens per second. A request spends one token; if
//! the bucket is empty the middleware responds 429.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Instant;

use crate::error::NanowebError;
use crate::http::types::{Request, Response, StatusCode};
use crate::middleware::{Middleware, MiddlewareOutcome};

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

pub struct RateLimitMiddleware {
    pub capacity: f64,
    pub refill_per_sec: f64,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimitMiddleware {
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            capacity,
            refill_per_sec,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn client_key(req: &Request) -> String {
        if let Some(fwd) = req.headers.get("x-forwarded-for") {
            if let Some(first) = fwd.split(',').next() {
                return first.trim().to_string();
            }
        }
        req.remote_addr
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn consume(&self, key: &str) -> bool {
        let mut map = self.buckets.lock().expect("rate-limit mutex poisoned");
        let now = Instant::now();
        let b = map.entry(key.to_string()).or_insert_with(|| Bucket {
            tokens: self.capacity,
            last_refill: now,
        });
        let elapsed = now.duration_since(b.last_refill).as_secs_f64();
        b.tokens = (b.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        b.last_refill = now;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

impl Default for RateLimitMiddleware {
    fn default() -> Self {
        Self::new(60.0, 30.0)
    }
}

impl Middleware for RateLimitMiddleware {
    fn name(&self) -> &'static str {
        "rate_limit"
    }

    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<MiddlewareOutcome, NanowebError>> + Send + 'a>>
    {
        Box::pin(async move {
            let key = Self::client_key(&req);
            if self.consume(&key) {
                Ok(MiddlewareOutcome::Continue(req))
            } else {
                let mut resp = Response::status(StatusCode::TOO_MANY_REQUESTS);
                resp.headers.set("retry-after", "1");
                Ok(MiddlewareOutcome::Respond(resp))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::Method;

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_when_empty() {
        let m = RateLimitMiddleware::new(2.0, 0.0);
        let req = || {
            let mut r = Request::new(Method::Get, "/");
            r.remote_addr = Some("1.2.3.4".into());
            r
        };
        assert!(matches!(
            m.before(req()).await.unwrap(),
            MiddlewareOutcome::Continue(_)
        ));
        assert!(matches!(
            m.before(req()).await.unwrap(),
            MiddlewareOutcome::Continue(_)
        ));
        match m.before(req()).await.unwrap() {
            MiddlewareOutcome::Respond(resp) => {
                assert_eq!(resp.status, StatusCode::TOO_MANY_REQUESTS)
            }
            _ => panic!("expected 429"),
        }
    }
}

use crate::http::request::Request;
use crate::http::response::Response;
use crate::middleware::Middleware;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub struct RateLimitMiddleware {
    inner: Mutex<RateLimitInner>,
}

struct RateLimitInner {
    buckets: HashMap<String, TokenBucket>,
    requests_per_second: u32,
    burst: u32,
}

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    max_tokens: f64,
    refill_rate: f64,
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

    fn allow(&mut self) -> bool {
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

impl RateLimitMiddleware {
    pub fn new(requests_per_second: u32, burst: u32) -> Self {
        RateLimitMiddleware {
            inner: Mutex::new(RateLimitInner {
                buckets: HashMap::new(),
                requests_per_second,
                burst,
            }),
        }
    }
}

impl Middleware for RateLimitMiddleware {
    fn handle(&self, req: Request, next: Box<dyn FnOnce(Request) -> Response + Send>) -> Response {
        let client_ip = req.remote_addr.clone().unwrap_or_else(|| "unknown".into());

        let allowed = {
            let mut inner = self.inner.lock().unwrap();
            let burst = inner.burst as f64;
            let rps = inner.requests_per_second as f64;
            let bucket = inner
                .buckets
                .entry(client_ip.clone())
                .or_insert_with(|| TokenBucket::new(burst, rps));
            bucket.allow()
        };

        if !allowed {
            let mut resp = Response::new(429);
            resp.headers.insert("retry-after".into(), "1".into());
            resp.body = b"Too Many Requests".to_vec();
            return resp;
        }

        next(req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_allows_normal_traffic() {
        let mw = RateLimitMiddleware::new(100, 100);
        let req = Request {
            method: crate::http::parser::Method::Get,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            query: std::collections::HashMap::new(),
            remote_addr: Some("127.0.0.1:1234".into()),
            request_id: "test".into(),
        };
        let resp = mw.handle(req, Box::new(|_| Response::text("ok")));
        assert_eq!(resp.status, 200);
    }
}

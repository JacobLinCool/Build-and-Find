use super::Middleware;
use crate::error::Result;
use crate::http::{Request, Response, StatusCode};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub struct RateLimit {
    capacity: f64,
    refill_per_s: f64,
    buckets: Mutex<HashMap<String, Bucket>>,
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

impl RateLimit {
    pub fn new(capacity: u32, refill_per_s: f64) -> Self {
        Self {
            capacity: capacity as f64,
            refill_per_s,
            buckets: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl Middleware for RateLimit {
    async fn before(&self, req: &mut Request) -> Result<Option<Response>> {
        let key = req.client_ip.clone().unwrap_or_else(|| "unknown".into());
        let mut buckets = self.buckets.lock().unwrap();
        let now = Instant::now();
        let bucket = buckets.entry(key).or_insert(Bucket {
            tokens: self.capacity,
            last: now,
        });
        bucket.tokens = (bucket.tokens
            + now.duration_since(bucket.last).as_secs_f64() * self.refill_per_s)
            .min(self.capacity);
        bucket.last = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(None)
        } else {
            Ok(Some(Response::new(
                StatusCode::TOO_MANY_REQUESTS,
                "rate limited",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;
    use std::collections::HashMap;

    #[tokio::test]
    async fn empty_bucket_rejects() {
        let mut req = Request {
            method: Method::Get,
            target: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HashMap::new(),
            body: vec![],
            params: HashMap::new(),
            client_ip: Some("127.0.0.1".into()),
            request_id: "r".into(),
        };
        assert!(
            RateLimit::new(0, 0.0)
                .before(&mut req)
                .await
                .unwrap()
                .is_some()
        );
    }
}

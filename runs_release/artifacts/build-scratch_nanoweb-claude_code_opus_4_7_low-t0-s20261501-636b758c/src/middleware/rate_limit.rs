use super::Middleware;
use crate::http::{Request, Response, StatusCode};
use crate::NanowebError;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct RateLimit {
    pub capacity: f64,
    pub refill_per_sec: f64,
    state: Mutex<HashMap<String, Bucket>>,
}

impl RateLimit {
    pub fn new(capacity: u32, refill_per_sec: f64) -> Self {
        Self {
            capacity: capacity as f64,
            refill_per_sec,
            state: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl Middleware for RateLimit {
    async fn before(&self, req: &mut Request) -> Result<Option<Response>, NanowebError> {
        let key = req
            .remote_addr
            .map(|a| a.ip().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        let bucket = state.entry(key).or_insert(Bucket {
            tokens: self.capacity,
            last: now,
        });
        let dt = now.duration_since(bucket.last).as_secs_f64();
        bucket.tokens = (bucket.tokens + dt * self.refill_per_sec).min(self.capacity);
        bucket.last = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(None)
        } else {
            let mut r = Response::new(StatusCode::TOO_MANY_REQUESTS);
            r.headers
                .insert("Content-Type", "text/plain; charset=utf-8");
            r.body = b"rate limit exceeded".to_vec();
            Ok(Some(r))
        }
    }
}

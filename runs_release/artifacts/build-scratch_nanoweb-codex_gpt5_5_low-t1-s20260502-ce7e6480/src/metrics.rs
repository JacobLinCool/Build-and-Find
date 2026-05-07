use crate::error::Result;
use crate::http::{Method, Response, StatusCode};
use prometheus::{Encoder, HistogramOpts, HistogramVec, IntCounterVec, Registry, TextEncoder};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct Metrics {
    registry: Arc<Registry>,
    requests: IntCounterVec,
    duration: HistogramVec,
}

impl Metrics {
    pub fn new() -> Result<Self> {
        let registry = Registry::new();
        let requests = IntCounterVec::new(
            prometheus::Opts::new("http_requests_total", "HTTP requests"),
            &["method", "status_class", "route"],
        )
        .unwrap();
        let duration = HistogramVec::new(
            HistogramOpts::new("http_request_duration_seconds", "HTTP request duration"),
            &["method", "route"],
        )
        .unwrap();
        registry.register(Box::new(requests.clone())).unwrap();
        registry.register(Box::new(duration.clone())).unwrap();
        Ok(Self {
            registry: Arc::new(registry),
            requests,
            duration,
        })
    }

    pub fn observe(&self, method: &Method, route: &str, status: StatusCode, started: Instant) {
        let class = format!("{}xx", status.0 / 100);
        self.requests
            .with_label_values(&[method.as_str(), &class, route])
            .inc();
        self.duration
            .with_label_values(&[method.as_str(), route])
            .observe(started.elapsed().as_secs_f64());
    }

    pub fn response(&self) -> Response {
        let mut buf = Vec::new();
        TextEncoder::new()
            .encode(&self.registry.gather(), &mut buf)
            .unwrap();
        let mut r = Response::new(StatusCode::OK, buf);
        r.headers
            .insert("content-type".into(), "text/plain; version=0.0.4".into());
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_text() {
        let m = Metrics::new().unwrap();
        m.observe(&Method::Get, "/", StatusCode::OK, Instant::now());
        assert!(
            String::from_utf8(m.response().body)
                .unwrap()
                .contains("http_requests_total")
        );
    }
}

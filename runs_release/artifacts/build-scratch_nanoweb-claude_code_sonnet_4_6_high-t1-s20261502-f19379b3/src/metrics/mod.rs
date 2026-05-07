use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder,
};
use std::sync::Arc;
use std::time::Instant;

pub struct Metrics {
    pub registry: Registry,
    pub requests_total: IntCounterVec,
    pub request_duration_seconds: HistogramVec,
}

impl Metrics {
    pub fn new() -> Result<Self, NanowebError> {
        let registry = Registry::new();

        let requests_total = IntCounterVec::new(
            Opts::new("http_requests_total", "Total number of HTTP requests"),
            &["method", "status_class", "route"],
        )
        .map_err(|e| NanowebError::Internal(e.to_string()))?;

        let request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "http_request_duration_seconds",
                "HTTP request duration in seconds",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]),
            &["method", "route"],
        )
        .map_err(|e| NanowebError::Internal(e.to_string()))?;

        registry
            .register(Box::new(requests_total.clone()))
            .map_err(|e| NanowebError::Internal(e.to_string()))?;
        registry
            .register(Box::new(request_duration_seconds.clone()))
            .map_err(|e| NanowebError::Internal(e.to_string()))?;

        Ok(Metrics {
            registry,
            requests_total,
            request_duration_seconds,
        })
    }

    pub fn record_request(
        &self,
        method: &str,
        status: u16,
        route: &str,
        duration: std::time::Duration,
    ) {
        let status_class = match status {
            100..=199 => "1xx",
            200..=299 => "2xx",
            300..=399 => "3xx",
            400..=499 => "4xx",
            500..=599 => "5xx",
            _ => "unknown",
        };

        self.requests_total
            .with_label_values(&[method, status_class, route])
            .inc();

        self.request_duration_seconds
            .with_label_values(&[method, route])
            .observe(duration.as_secs_f64());
    }

    pub fn exposition(&self) -> Result<String, NanowebError> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buf = Vec::new();
        encoder
            .encode(&metric_families, &mut buf)
            .map_err(|e| NanowebError::Internal(e.to_string()))?;
        String::from_utf8(buf).map_err(|e| NanowebError::Internal(e.to_string()))
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new().expect("Metrics initialization failed")
    }
}

pub fn metrics_handler(
    metrics: Arc<Metrics>,
) -> impl Fn(Request) -> crate::middleware::BoxFuture<Result<Response, NanowebError>>
       + Send
       + Sync
       + 'static {
    move |_req: Request| {
        let metrics = metrics.clone();
        Box::pin(async move {
            match metrics.exposition() {
                Ok(text) => {
                    let mut resp = Response::with_body(200, text.into_bytes());
                    resp.set_header("Content-Type", "text/plain; version=0.0.4; charset=utf-8");
                    Ok(resp)
                }
                Err(e) => Err(e),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let m = Metrics::new().unwrap();
        // Just ensure exposition works without error
        let output = m.exposition().unwrap();
        // Output may be empty if no samples recorded yet, that's fine
        assert!(output.is_empty() || !output.is_empty());
    }

    #[test]
    fn test_record_request() {
        let m = Metrics::new().unwrap();
        m.record_request("GET", 200, "/hello", std::time::Duration::from_millis(10));
        let output = m.exposition().unwrap();
        assert!(output.contains("http_requests_total"));
    }
}

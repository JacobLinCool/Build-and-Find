use crate::error::NanowebError;
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry,
    TextEncoder,
};
use std::sync::Arc;

pub struct Metrics {
    pub registry: Registry,
    pub request_count: IntCounterVec,
    pub request_duration: HistogramVec,
}

impl Metrics {
    pub fn new() -> Result<Arc<Self>, NanowebError> {
        let registry = Registry::new();

        let request_count = IntCounterVec::new(
            Opts::new("nanoweb_requests_total", "Total number of HTTP requests"),
            &["method", "status", "route"],
        )
        .map_err(|e| NanowebError::InternalServer(e.to_string()))?;

        let request_duration = HistogramVec::new(
            HistogramOpts::new(
                "nanoweb_request_duration_seconds",
                "HTTP request duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
            &["method", "route"],
        )
        .map_err(|e| NanowebError::InternalServer(e.to_string()))?;

        registry
            .register(Box::new(request_count.clone()))
            .map_err(|e| NanowebError::InternalServer(e.to_string()))?;
        registry
            .register(Box::new(request_duration.clone()))
            .map_err(|e| NanowebError::InternalServer(e.to_string()))?;

        Ok(Arc::new(Metrics {
            registry,
            request_count,
            request_duration,
        }))
    }

    pub fn record_request(&self, method: &str, status: u16, route: &str, duration_secs: f64) {
        self.request_count
            .with_label_values(&[method, &status.to_string(), route])
            .inc();
        self.request_duration
            .with_label_values(&[method, route])
            .observe(duration_secs);
    }

    pub fn render(&self) -> Result<String, NanowebError> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut output = Vec::new();
        encoder
            .encode(&metric_families, &mut output)
            .map_err(|e| NanowebError::InternalServer(e.to_string()))?;
        String::from_utf8(output).map_err(NanowebError::Utf8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = Metrics::new().unwrap();
        metrics.record_request("GET", 200, "/", 0.01);
        let rendered = metrics.render().unwrap();
        assert!(rendered.contains("nanoweb_requests_total"));
    }
}

use std::sync::OnceLock;
use std::time::Duration;

use prometheus::{Encoder, HistogramVec, IntCounterVec, Registry, TextEncoder};

use crate::error::{NanowebError, Result};
use crate::http::Method;

struct MetricSet {
    registry: Registry,
    requests: IntCounterVec,
    duration: HistogramVec,
}

static METRICS: OnceLock<MetricSet> = OnceLock::new();

fn metrics() -> &'static MetricSet {
    METRICS.get_or_init(|| {
        let registry = Registry::new();
        let requests = IntCounterVec::new(
            prometheus::Opts::new("nanoweb_http_requests_total", "HTTP requests"),
            &["method", "status_class", "route"],
        )
        .expect("counter vec");
        let duration = HistogramVec::new(
            prometheus::HistogramOpts::new(
                "nanoweb_http_request_duration_seconds",
                "Request duration in seconds",
            ),
            &["method", "route"],
        )
        .expect("histogram vec");
        registry
            .register(Box::new(requests.clone()))
            .expect("register counter");
        registry
            .register(Box::new(duration.clone()))
            .expect("register histogram");
        MetricSet {
            registry,
            requests,
            duration,
        }
    })
}

pub fn record_request(method: &Method, status: u16, route: &str, duration: Duration) {
    let status_class = format!("{}", status / 100);
    metrics()
        .requests
        .with_label_values(&[method.as_str(), &status_class, route])
        .inc();
    metrics()
        .duration
        .with_label_values(&[method.as_str(), route])
        .observe(duration.as_secs_f64());
}

pub fn gather() -> Result<String> {
    let metric_families = metrics().registry.gather();
    let mut buffer = Vec::new();
    TextEncoder::new()
        .encode(&metric_families, &mut buffer)
        .map_err(|err| NanowebError::Metrics(err.to_string()))?;
    String::from_utf8(buffer).map_err(|err| NanowebError::Metrics(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn renders_text_exposition() {
        record_request(&Method::Get, 200, "/healthz", Duration::from_millis(1));
        let body = gather().unwrap();
        assert!(body.contains("nanoweb_http_requests_total"));
    }
}

use prometheus::{Encoder, Histogram, HistogramOpts, IntCounter, Registry, TextEncoder};

#[derive(Clone)]
pub struct Metrics {
    registry: Registry,
    pub requests_total: IntCounter,
    pub request_duration: Histogram,
    pub responses_by_status: IntCounter,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let requests_total = IntCounter::with_opts(prometheus::opts!(
            "nanoweb_http_requests_total",
            "Total number of HTTP requests"
        ))
        .unwrap();

        let request_duration = Histogram::with_opts(
            HistogramOpts::new(
                "nanoweb_http_request_duration_seconds",
                "Request duration in seconds",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
            ]),
        )
        .unwrap();

        let responses_by_status = IntCounter::with_opts(prometheus::opts!(
            "nanoweb_http_responses_by_status_total",
            "Total responses by status class"
        ))
        .unwrap();

        registry.register(Box::new(requests_total.clone())).unwrap();
        registry
            .register(Box::new(request_duration.clone()))
            .unwrap();
        registry
            .register(Box::new(responses_by_status.clone()))
            .unwrap();

        Metrics {
            registry,
            requests_total,
            request_duration,
            responses_by_status,
        }
    }

    pub fn record_request(&self, _method: &str, _path: &str, _status: u16, duration_secs: f64) {
        self.requests_total.inc();
        self.request_duration.observe(duration_secs);
        self.responses_by_status.inc();
    }

    pub fn render(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

pub fn metrics_handler(metrics: &Metrics) -> crate::http::response::Response {
    let body = metrics.render();
    let mut resp = crate::http::response::Response::ok();
    resp.headers.insert(
        "content-type".into(),
        "text/plain; version=0.0.4; charset=utf-8".into(),
    );
    resp.body = body.into_bytes();
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_render() {
        let metrics = Metrics::new();
        metrics.record_request("GET", "/", 200, 0.01);
        let output = metrics.render();
        assert!(output.contains("nanoweb_http_requests_total"));
    }
}

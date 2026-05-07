use prometheus::{Encoder, HistogramOpts, HistogramVec, IntCounterVec, Registry, TextEncoder};
use std::sync::OnceLock;

pub struct Metrics {
    registry: Registry,
    requests: IntCounterVec,
    duration: HistogramVec,
}

static METRICS: OnceLock<Metrics> = OnceLock::new();

impl Metrics {
    pub fn global() -> &'static Metrics {
        METRICS.get_or_init(|| {
            let registry = Registry::new();
            let requests = IntCounterVec::new(
                prometheus::Opts::new("http_requests_total", "HTTP requests"),
                &["method", "status_class", "route"],
            )
            .unwrap();
            let duration = HistogramVec::new(
                HistogramOpts::new("http_request_duration_seconds", "Request duration"),
                &["method", "route"],
            )
            .unwrap();
            registry.register(Box::new(requests.clone())).unwrap();
            registry.register(Box::new(duration.clone())).unwrap();
            Metrics {
                registry,
                requests,
                duration,
            }
        })
    }
    pub fn observe(&self, method: &str, status: u16, route: &str, seconds: f64) {
        self.requests
            .with_label_values(&[method, &format!("{}xx", status / 100), route])
            .inc();
        self.duration
            .with_label_values(&[method, route])
            .observe(seconds);
    }
    pub fn render(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let encoder = TextEncoder::new();
        let mf = self.registry.gather();
        encoder.encode(&mf, &mut buf).unwrap();
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders() {
        Metrics::global().observe("GET", 200, "/x", 0.1);
        assert!(!Metrics::global().render().is_empty());
    }
}

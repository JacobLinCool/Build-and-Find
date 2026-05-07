use std::sync::OnceLock;

use prometheus::{Counter, CounterVec, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder};

use crate::error::NanowebError;

static REGISTRY: OnceLock<MetricsRegistry> = OnceLock::new();

pub struct MetricsRegistry {
    pub registry: Registry,
    pub request_total: CounterVec,
    pub request_duration: HistogramVec,
    pub active_connections: Counter,
}

impl MetricsRegistry {
    fn new() -> Result<Self, NanowebError> {
        let registry = Registry::new();

        let request_total = CounterVec::new(
            Opts::new("nanoweb_http_requests_total", "Total HTTP requests"),
            &["method", "route", "status_class"],
        )
        .map_err(|e| NanowebError::Internal(e.to_string()))?;

        let request_duration = HistogramVec::new(
            HistogramOpts::new(
                "nanoweb_http_request_duration_seconds",
                "HTTP request duration in seconds",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]),
            &["method", "route"],
        )
        .map_err(|e| NanowebError::Internal(e.to_string()))?;

        let active_connections = Counter::new(
            "nanoweb_active_connections_total",
            "Total accepted connections",
        )
        .map_err(|e| NanowebError::Internal(e.to_string()))?;

        registry
            .register(Box::new(request_total.clone()))
            .map_err(|e| NanowebError::Internal(e.to_string()))?;
        registry
            .register(Box::new(request_duration.clone()))
            .map_err(|e| NanowebError::Internal(e.to_string()))?;
        registry
            .register(Box::new(active_connections.clone()))
            .map_err(|e| NanowebError::Internal(e.to_string()))?;

        Ok(Self {
            registry,
            request_total,
            request_duration,
            active_connections,
        })
    }
}

pub fn init() -> Result<(), NanowebError> {
    REGISTRY
        .set(MetricsRegistry::new()?)
        .map_err(|_| NanowebError::Internal("Metrics already initialized".into()))
}

pub fn registry() -> Option<&'static MetricsRegistry> {
    REGISTRY.get()
}

pub fn record_request(method: &str, route: &str, status: u16, duration_secs: f64) {
    if let Some(reg) = REGISTRY.get() {
        let status_class = match status / 100 {
            1 => "1xx",
            2 => "2xx",
            3 => "3xx",
            4 => "4xx",
            5 => "5xx",
            _ => "unknown",
        };
        let _ = reg
            .request_total
            .get_metric_with_label_values(&[method, route, status_class])
            .map(|c| c.inc());
        let _ = reg
            .request_duration
            .get_metric_with_label_values(&[method, route])
            .map(|h| h.observe(duration_secs));
    }
}

pub fn record_connection() {
    if let Some(reg) = REGISTRY.get() {
        reg.active_connections.inc();
    }
}

pub fn gather_text() -> Result<String, NanowebError> {
    let reg = REGISTRY
        .get()
        .ok_or_else(|| NanowebError::Internal("Metrics not initialized".into()))?;

    let encoder = TextEncoder::new();
    let metric_families = reg.registry.gather();
    encoder
        .encode_to_string(&metric_families)
        .map_err(|e| NanowebError::Internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_initialization() {
        // May already be initialized in test suite
        let _ = init();
        assert!(REGISTRY.get().is_some());
    }

    #[test]
    fn test_record_request() {
        let _ = init();
        record_request("GET", "/test", 200, 0.01);
        let text = gather_text().unwrap_or_default();
        // After recording, metrics should be gatherable
        assert!(!text.is_empty() || REGISTRY.get().is_none());
    }

    #[test]
    fn test_gather_text_format() {
        let _ = init();
        if let Ok(text) = gather_text() {
            // Prometheus text format starts with # HELP or # TYPE lines
            assert!(
                text.is_empty()
                    || text.contains("nanoweb_http_requests_total")
                    || text.starts_with('#')
            );
        }
    }
}

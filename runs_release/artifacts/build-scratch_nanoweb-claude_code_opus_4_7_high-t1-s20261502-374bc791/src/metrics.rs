//! Prometheus-text-format metrics registry.
//!
//! Public API: [`Metrics::record_request`] is the only function the rest of
//! the crate calls. The router and the metrics endpoint go through it. No
//! other module owns counters.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

const DURATION_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

#[derive(Default)]
struct HistogramData {
    buckets: Vec<u64>,
    sum: f64,
    count: u64,
}

impl HistogramData {
    fn new() -> Self {
        Self {
            buckets: vec![0; DURATION_BUCKETS.len()],
            sum: 0.0,
            count: 0,
        }
    }

    fn observe(&mut self, v: f64) {
        for (i, b) in DURATION_BUCKETS.iter().enumerate() {
            if v <= *b {
                self.buckets[i] += 1;
            }
        }
        self.sum += v;
        self.count += 1;
    }
}

#[derive(Default)]
struct Registry {
    /// (method, status_class, route) -> count
    request_counts: HashMap<(String, String, String), u64>,
    /// (method, route) -> histogram
    duration: HashMap<(String, String), HistogramData>,
}

pub struct Metrics {
    inner: Mutex<Registry>,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Registry::default()),
        }
    }

    pub fn record_request(
        &self,
        method: &str,
        status_class: &str,
        route: &str,
        duration: Duration,
    ) {
        let mut g = self.inner.lock().expect("metrics lock");
        let key = (
            method.to_string(),
            status_class.to_string(),
            route.to_string(),
        );
        *g.request_counts.entry(key).or_insert(0) += 1;
        let dkey = (method.to_string(), route.to_string());
        g.duration
            .entry(dkey)
            .or_insert_with(HistogramData::new)
            .observe(duration.as_secs_f64());
    }

    /// Render the registry in Prometheus text exposition format.
    pub fn render(&self) -> String {
        let g = self.inner.lock().expect("metrics lock");
        let mut out = String::new();
        out.push_str("# HELP nanoweb_http_requests_total Total HTTP requests.\n");
        out.push_str("# TYPE nanoweb_http_requests_total counter\n");
        for ((method, class, route), count) in &g.request_counts {
            out.push_str(&format!(
                "nanoweb_http_requests_total{{method=\"{}\",status=\"{}\",route=\"{}\"}} {}\n",
                escape(method),
                escape(class),
                escape(route),
                count
            ));
        }
        out.push_str("# HELP nanoweb_http_request_duration_seconds Request latency in seconds.\n");
        out.push_str("# TYPE nanoweb_http_request_duration_seconds histogram\n");
        for ((method, route), hist) in &g.duration {
            for (i, b) in DURATION_BUCKETS.iter().enumerate() {
                out.push_str(&format!(
                    "nanoweb_http_request_duration_seconds_bucket{{method=\"{}\",route=\"{}\",le=\"{}\"}} {}\n",
                    escape(method),
                    escape(route),
                    b,
                    hist.buckets[i]
                ));
            }
            out.push_str(&format!(
                "nanoweb_http_request_duration_seconds_bucket{{method=\"{}\",route=\"{}\",le=\"+Inf\"}} {}\n",
                escape(method),
                escape(route),
                hist.count
            ));
            out.push_str(&format!(
                "nanoweb_http_request_duration_seconds_sum{{method=\"{}\",route=\"{}\"}} {}\n",
                escape(method),
                escape(route),
                hist.sum
            ));
            out.push_str(&format!(
                "nanoweb_http_request_duration_seconds_count{{method=\"{}\",route=\"{}\"}} {}\n",
                escape(method),
                escape(route),
                hist.count
            ));
        }
        out
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn records_and_renders() {
        let m = Metrics::new();
        m.record_request("GET", "2xx", "/x", Duration::from_millis(10));
        m.record_request("GET", "2xx", "/x", Duration::from_millis(50));
        let s = m.render();
        assert!(s.contains("nanoweb_http_requests_total"));
        assert!(s.contains("nanoweb_http_request_duration_seconds_count"));
        assert!(s.contains("route=\"/x\""));
    }
}

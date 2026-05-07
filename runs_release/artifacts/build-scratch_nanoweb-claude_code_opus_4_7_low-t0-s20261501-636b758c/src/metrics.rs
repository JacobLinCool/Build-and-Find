//! Prometheus text-exposition metrics.

use crate::http::Method;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct Metrics {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    requests: HashMap<(Method, String, String), u64>, // (method, route, status_class)
    duration_buckets: HashMap<String, [u64; BUCKETS.len()]>, // per route
    duration_sum: HashMap<String, f64>,
    duration_count: HashMap<String, u64>,
}

const BUCKETS: [f64; 11] = [
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
];

impl Default for Metrics {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, method: Method, route: &str, status_class: &str, duration_s: f64) {
        let mut inner = self.inner.lock().unwrap();
        *inner
            .requests
            .entry((method, route.to_string(), status_class.to_string()))
            .or_insert(0) += 1;
        let buckets = inner
            .duration_buckets
            .entry(route.to_string())
            .or_insert([0u64; BUCKETS.len()]);
        for (i, b) in BUCKETS.iter().enumerate() {
            if duration_s <= *b {
                buckets[i] += 1;
            }
        }
        *inner.duration_sum.entry(route.to_string()).or_insert(0.0) += duration_s;
        *inner.duration_count.entry(route.to_string()).or_insert(0) += 1;
    }

    pub fn render(&self) -> String {
        let inner = self.inner.lock().unwrap();
        let mut out = String::new();
        out.push_str("# HELP nanoweb_http_requests_total Total HTTP requests.\n");
        out.push_str("# TYPE nanoweb_http_requests_total counter\n");
        for ((method, route, class), count) in &inner.requests {
            out.push_str(&format!(
                "nanoweb_http_requests_total{{method=\"{}\",route=\"{}\",status=\"{}\"}} {}\n",
                method, route, class, count
            ));
        }
        out.push_str("# HELP nanoweb_http_request_duration_seconds Request duration.\n");
        out.push_str("# TYPE nanoweb_http_request_duration_seconds histogram\n");
        for (route, buckets) in &inner.duration_buckets {
            let mut cumulative = 0u64;
            for (i, b) in BUCKETS.iter().enumerate() {
                cumulative += buckets[i];
                out.push_str(&format!(
                    "nanoweb_http_request_duration_seconds_bucket{{route=\"{}\",le=\"{}\"}} {}\n",
                    route, b, cumulative
                ));
            }
            let total = inner.duration_count.get(route).copied().unwrap_or(0);
            out.push_str(&format!(
                "nanoweb_http_request_duration_seconds_bucket{{route=\"{}\",le=\"+Inf\"}} {}\n",
                route, total
            ));
            out.push_str(&format!(
                "nanoweb_http_request_duration_seconds_sum{{route=\"{}\"}} {}\n",
                route,
                inner.duration_sum.get(route).copied().unwrap_or(0.0)
            ));
            out.push_str(&format!(
                "nanoweb_http_request_duration_seconds_count{{route=\"{}\"}} {}\n",
                route, total
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_renders() {
        let m = Metrics::new();
        m.record(Method::Get, "/", "2xx", 0.01);
        let s = m.render();
        assert!(s.contains("nanoweb_http_requests_total"));
        assert!(s.contains("nanoweb_http_request_duration_seconds_bucket"));
    }
}

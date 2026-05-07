//! Prometheus text-exposition metrics.

use crate::http::{Method, Response};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
struct Inner {
    requests: BTreeMap<(String, String, String), u64>,
    duration_buckets: BTreeMap<String, [u64; 9]>,
    duration_sum: BTreeMap<String, f64>,
    duration_count: BTreeMap<String, u64>,
}

const BUCKETS: [f64; 9] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5];

#[derive(Clone)]
pub struct Metrics {
    inner: Arc<Mutex<Inner>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
        }
    }

    pub fn record_request(&self, method: Method, status: u16, route: &str, dur: Duration) {
        let mut g = self.inner.lock();
        let class = format!("{}xx", status / 100);
        let key = (method.as_str().to_string(), class, route.to_string());
        *g.requests.entry(key).or_insert(0) += 1;
        let secs = dur.as_secs_f64();
        let entry = g
            .duration_buckets
            .entry(route.to_string())
            .or_insert([0; 9]);
        for (i, b) in BUCKETS.iter().enumerate() {
            if secs <= *b {
                entry[i] += 1;
            }
        }
        *g.duration_sum.entry(route.to_string()).or_insert(0.0) += secs;
        *g.duration_count.entry(route.to_string()).or_insert(0) += 1;
    }

    pub fn render(&self) -> String {
        let g = self.inner.lock();
        let mut out = String::new();
        writeln!(
            out,
            "# HELP nanoweb_http_requests_total Total HTTP requests"
        )
        .ok();
        writeln!(out, "# TYPE nanoweb_http_requests_total counter").ok();
        for ((method, class, route), count) in g.requests.iter() {
            writeln!(
                out,
                "nanoweb_http_requests_total{{method=\"{}\",status=\"{}\",route=\"{}\"}} {}",
                method, class, route, count
            )
            .ok();
        }
        writeln!(
            out,
            "# HELP nanoweb_http_request_duration_seconds Request duration"
        )
        .ok();
        writeln!(
            out,
            "# TYPE nanoweb_http_request_duration_seconds histogram"
        )
        .ok();
        for (route, buckets) in g.duration_buckets.iter() {
            let mut cumulative = 0u64;
            for (i, b) in BUCKETS.iter().enumerate() {
                cumulative += buckets[i];
                writeln!(
                    out,
                    "nanoweb_http_request_duration_seconds_bucket{{route=\"{}\",le=\"{}\"}} {}",
                    route, b, cumulative
                )
                .ok();
            }
            let count = g.duration_count.get(route).copied().unwrap_or(0);
            let sum = g.duration_sum.get(route).copied().unwrap_or(0.0);
            writeln!(
                out,
                "nanoweb_http_request_duration_seconds_bucket{{route=\"{}\",le=\"+Inf\"}} {}",
                route, count
            )
            .ok();
            writeln!(
                out,
                "nanoweb_http_request_duration_seconds_sum{{route=\"{}\"}} {}",
                route, sum
            )
            .ok();
            writeln!(
                out,
                "nanoweb_http_request_duration_seconds_count{{route=\"{}\"}} {}",
                route, count
            )
            .ok();
        }
        out
    }

    pub fn response(&self) -> Response {
        let body = self.render();
        let mut r = Response::ok();
        r.headers
            .insert("Content-Type", "text/plain; version=0.0.4; charset=utf-8");
        r.body = body.into_bytes();
        r
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn render_basic() {
        let m = Metrics::new();
        m.record_request(Method::Get, 200, "/x", Duration::from_millis(10));
        let s = m.render();
        assert!(s.contains("nanoweb_http_requests_total"));
        assert!(s.contains("nanoweb_http_request_duration_seconds_bucket"));
    }
}

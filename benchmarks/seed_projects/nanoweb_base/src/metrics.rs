//! Prometheus text-exposition metrics.
//!
//! This is a hand-written formatter — enough to expose the two metrics
//! the framework cares about without pulling in the full `prometheus`
//! crate.
//!
//! * `http_requests_total{method,status,route}` — counter
//! * `http_request_duration_seconds` — histogram

use std::collections::HashMap;
use std::sync::Mutex;

/// Default histogram buckets (seconds). Values are monotonically
/// increasing; `+Inf` is implicit.
pub const DEFAULT_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

#[derive(Default)]
struct CounterVec {
    /// key: method|status|route
    values: HashMap<String, u64>,
}

struct HistogramVec {
    buckets: Vec<f64>,
    /// per-label-set counts-per-bucket; one extra slot at the end for
    /// observations above all buckets.
    counts: HashMap<String, Vec<u64>>,
    sums: HashMap<String, f64>,
    totals: HashMap<String, u64>,
}

impl HistogramVec {
    fn new(buckets: Vec<f64>) -> Self {
        Self {
            buckets,
            counts: HashMap::new(),
            sums: HashMap::new(),
            totals: HashMap::new(),
        }
    }

    fn observe(&mut self, key: &str, v: f64) {
        let slot = self
            .counts
            .entry(key.to_string())
            .or_insert_with(|| vec![0; self.buckets.len() + 1]);
        let mut placed = false;
        for (i, b) in self.buckets.iter().enumerate() {
            if v <= *b {
                slot[i] += 1;
                placed = true;
                break;
            }
        }
        if !placed {
            let last = slot.len() - 1;
            slot[last] += 1;
        }
        *self.sums.entry(key.to_string()).or_insert(0.0) += v;
        *self.totals.entry(key.to_string()).or_insert(0) += 1;
    }
}

pub struct Metrics {
    requests: Mutex<CounterVec>,
    durations: Mutex<HistogramVec>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new(DEFAULT_BUCKETS.to_vec())
    }
}

impl Metrics {
    pub fn new(buckets: Vec<f64>) -> Self {
        Self {
            requests: Mutex::new(CounterVec::default()),
            durations: Mutex::new(HistogramVec::new(buckets)),
        }
    }

    pub fn observe(&self, method: &str, status: u16, route: &str, duration_seconds: f64) {
        let key = format!("{}|{}|{}", method, status, route);
        if let Ok(mut r) = self.requests.lock() {
            *r.values.entry(key.clone()).or_insert(0) += 1;
        }
        let d_key = format!("{}|{}", method, route);
        if let Ok(mut d) = self.durations.lock() {
            d.observe(&d_key, duration_seconds);
        }
    }

    /// Produce the prometheus text-format exposition.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("# HELP http_requests_total Total number of HTTP requests handled.\n");
        out.push_str("# TYPE http_requests_total counter\n");
        if let Ok(r) = self.requests.lock() {
            for (k, v) in r.values.iter() {
                let mut parts = k.splitn(3, '|');
                let (method, status, route) = (
                    parts.next().unwrap_or(""),
                    parts.next().unwrap_or(""),
                    parts.next().unwrap_or(""),
                );
                out.push_str(&format!(
                    "http_requests_total{{method=\"{}\",status=\"{}\",route=\"{}\"}} {}\n",
                    escape(method),
                    escape(status),
                    escape(route),
                    v
                ));
            }
        }

        out.push_str("# HELP http_request_duration_seconds Observed request durations.\n");
        out.push_str("# TYPE http_request_duration_seconds histogram\n");
        if let Ok(d) = self.durations.lock() {
            for (k, counts) in d.counts.iter() {
                let mut parts = k.splitn(2, '|');
                let (method, route) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                let total: u64 = counts.iter().sum();
                let mut cumulative = 0u64;
                for (i, b) in d.buckets.iter().enumerate() {
                    cumulative += counts[i];
                    out.push_str(&format!(
                        "http_request_duration_seconds_bucket{{method=\"{}\",route=\"{}\",le=\"{}\"}} {}\n",
                        escape(method),
                        escape(route),
                        b,
                        cumulative
                    ));
                }
                out.push_str(&format!(
                    "http_request_duration_seconds_bucket{{method=\"{}\",route=\"{}\",le=\"+Inf\"}} {}\n",
                    escape(method),
                    escape(route),
                    total
                ));
                if let Some(sum) = d.sums.get(k) {
                    out.push_str(&format!(
                        "http_request_duration_seconds_sum{{method=\"{}\",route=\"{}\"}} {}\n",
                        escape(method),
                        escape(route),
                        sum
                    ));
                }
                if let Some(count) = d.totals.get(k) {
                    out.push_str(&format!(
                        "http_request_duration_seconds_count{{method=\"{}\",route=\"{}\"}} {}\n",
                        escape(method),
                        escape(route),
                        count
                    ));
                }
            }
        }

        out
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_counter_and_histogram() {
        let m = Metrics::default();
        m.observe("GET", 200, "/x", 0.01);
        m.observe("GET", 200, "/x", 0.5);
        m.observe("POST", 500, "/y", 2.0);
        let out = m.render();
        assert!(out.contains("http_requests_total{method=\"GET\",status=\"200\",route=\"/x\"} 2"));
        assert!(out.contains("http_request_duration_seconds_bucket"));
        assert!(out.contains("http_request_duration_seconds_count"));
    }
}

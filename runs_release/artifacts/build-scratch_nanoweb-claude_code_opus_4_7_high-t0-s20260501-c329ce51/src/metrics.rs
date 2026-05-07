use crate::http::Method;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::Arc;

const HISTOGRAM_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

#[derive(Default)]
struct CounterMap {
    inner: BTreeMap<(String, String, String), u64>, // (method, status_class, route) -> count
}

#[derive(Default)]
struct Histogram {
    bucket_counts: Vec<u64>,
    sum: f64,
    count: u64,
}

impl Histogram {
    fn observe(&mut self, secs: f64) {
        if self.bucket_counts.is_empty() {
            self.bucket_counts = vec![0; HISTOGRAM_BUCKETS.len()];
        }
        for (i, b) in HISTOGRAM_BUCKETS.iter().enumerate() {
            if secs <= *b {
                self.bucket_counts[i] += 1;
            }
        }
        self.sum += secs;
        self.count += 1;
    }
}

#[derive(Default)]
struct State {
    counters: CounterMap,
    histogram: Histogram,
}

#[derive(Clone, Default)]
pub struct Metrics {
    state: Arc<Mutex<State>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, method: Method, status: u16, route: &str, duration_secs: f64) {
        let mut s = self.state.lock();
        let class = match status / 100 {
            1 => "1xx",
            2 => "2xx",
            3 => "3xx",
            4 => "4xx",
            5 => "5xx",
            _ => "unknown",
        };
        let key = (
            method.as_str().to_string(),
            class.to_string(),
            route.to_string(),
        );
        *s.counters.inner.entry(key).or_insert(0) += 1;
        s.histogram.observe(duration_secs);
    }

    pub fn render_prometheus(&self) -> String {
        let s = self.state.lock();
        let mut out = String::new();
        out.push_str("# HELP nanoweb_http_requests_total Total HTTP requests\n");
        out.push_str("# TYPE nanoweb_http_requests_total counter\n");
        for ((method, class, route), count) in s.counters.inner.iter() {
            out.push_str(&format!(
                "nanoweb_http_requests_total{{method=\"{}\",status_class=\"{}\",route=\"{}\"}} {}\n",
                escape(method),
                escape(class),
                escape(route),
                count
            ));
        }
        out.push_str("# HELP nanoweb_http_request_duration_seconds HTTP request duration\n");
        out.push_str("# TYPE nanoweb_http_request_duration_seconds histogram\n");
        if !s.histogram.bucket_counts.is_empty() {
            for (i, b) in HISTOGRAM_BUCKETS.iter().enumerate() {
                out.push_str(&format!(
                    "nanoweb_http_request_duration_seconds_bucket{{le=\"{}\"}} {}\n",
                    b, s.histogram.bucket_counts[i]
                ));
            }
        }
        out.push_str(&format!(
            "nanoweb_http_request_duration_seconds_bucket{{le=\"+Inf\"}} {}\n",
            s.histogram.count
        ));
        out.push_str(&format!(
            "nanoweb_http_request_duration_seconds_sum {}\n",
            s.histogram.sum
        ));
        out.push_str(&format!(
            "nanoweb_http_request_duration_seconds_count {}\n",
            s.histogram.count
        ));
        out
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

    #[test]
    fn records_and_renders() {
        let m = Metrics::new();
        m.record(Method::Get, 200, "/foo", 0.01);
        m.record(Method::Get, 500, "/foo", 0.5);
        let out = m.render_prometheus();
        assert!(out.contains("nanoweb_http_requests_total"));
        assert!(out.contains("status_class=\"2xx\""));
        assert!(out.contains("status_class=\"5xx\""));
        assert!(out.contains("nanoweb_http_request_duration_seconds_bucket"));
    }
}

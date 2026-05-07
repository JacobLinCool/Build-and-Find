use crate::http::types::{Method, StatusCode};
use std::{
    collections::HashMap,
    fmt::Write,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Default)]
pub struct Metrics {
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    request_counts: HashMap<RequestKey, u64>,
    request_histograms: HashMap<RequestKey, Histogram>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct RequestKey {
    method: String,
    status_class: String,
    route: String,
}

#[derive(Clone)]
struct Histogram {
    buckets: Vec<f64>,
    counts: Vec<u64>,
    sum: f64,
    total: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        let buckets = vec![
            0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ];
        Self {
            counts: vec![0; buckets.len() + 1],
            buckets,
            sum: 0.0,
            total: 0,
        }
    }
}

impl Metrics {
    pub fn observe_request(
        &self,
        method: Method,
        status: StatusCode,
        route: &str,
        duration: Duration,
    ) {
        let key = RequestKey {
            method: method.to_string(),
            status_class: format!("{}", status.class()),
            route: route.to_string(),
        };
        let mut state = self.state.lock().unwrap();
        *state.request_counts.entry(key.clone()).or_insert(0) += 1;
        let histogram = state.request_histograms.entry(key).or_default();
        histogram.observe(duration.as_secs_f64());
    }

    pub fn render(&self) -> String {
        let state = self.state.lock().unwrap();
        let mut out = String::new();
        out.push_str("# HELP nanoweb_http_requests_total Total HTTP requests\n");
        out.push_str("# TYPE nanoweb_http_requests_total counter\n");
        for (key, count) in &state.request_counts {
            let _ = writeln!(
                out,
                "nanoweb_http_requests_total{{method=\"{}\",status_class=\"{}\",route=\"{}\"}} {}",
                escape_label(&key.method),
                escape_label(&key.status_class),
                escape_label(&key.route),
                count
            );
        }
        out.push_str("# HELP nanoweb_http_request_duration_seconds Request duration in seconds\n");
        out.push_str("# TYPE nanoweb_http_request_duration_seconds histogram\n");
        for (key, histogram) in &state.request_histograms {
            let mut cumulative = 0u64;
            for (bucket, count) in histogram.buckets.iter().zip(histogram.counts.iter()) {
                cumulative += count;
                let _ = writeln!(
                    out,
                    "nanoweb_http_request_duration_seconds_bucket{{method=\"{}\",status_class=\"{}\",route=\"{}\",le=\"{}\"}} {}",
                    escape_label(&key.method),
                    escape_label(&key.status_class),
                    escape_label(&key.route),
                    bucket,
                    cumulative
                );
            }
            cumulative += histogram.counts.last().copied().unwrap_or(0);
            let _ = writeln!(
                out,
                "nanoweb_http_request_duration_seconds_bucket{{method=\"{}\",status_class=\"{}\",route=\"{}\",le=\"+Inf\"}} {}",
                escape_label(&key.method),
                escape_label(&key.status_class),
                escape_label(&key.route),
                cumulative
            );
            let _ = writeln!(
                out,
                "nanoweb_http_request_duration_seconds_sum{{method=\"{}\",status_class=\"{}\",route=\"{}\"}} {}",
                escape_label(&key.method),
                escape_label(&key.status_class),
                escape_label(&key.route),
                histogram.sum
            );
            let _ = writeln!(
                out,
                "nanoweb_http_request_duration_seconds_count{{method=\"{}\",status_class=\"{}\",route=\"{}\"}} {}",
                escape_label(&key.method),
                escape_label(&key.status_class),
                escape_label(&key.route),
                histogram.total
            );
        }
        out
    }
}

impl Histogram {
    fn observe(&mut self, value: f64) {
        self.sum += value;
        self.total += 1;
        for (idx, bucket) in self.buckets.iter().enumerate() {
            if value <= *bucket {
                self.counts[idx] += 1;
                return;
            }
        }
        if let Some(last) = self.counts.last_mut() {
            *last += 1;
        }
    }
}

fn escape_label(value: &str) -> String {
    value
        .replace('\\', r"\\")
        .replace('\n', r"\n")
        .replace('"', r#"\""#)
}

#[cfg(test)]
mod tests {
    use super::Metrics;
    use crate::http::types::{Method, StatusCode};
    use std::time::Duration;

    #[test]
    fn render_text() {
        let metrics = Metrics::default();
        metrics.observe_request(Method::GET, StatusCode::Ok, "/", Duration::from_millis(25));
        let rendered = metrics.render();
        assert!(rendered.contains("nanoweb_http_requests_total"));
    }
}

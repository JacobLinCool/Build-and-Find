use std::{collections::HashMap, sync::Mutex, time::Duration};

const BUCKETS: [f64; 8] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 1.0, 5.0];

#[derive(Default)]
struct Histogram {
    buckets: [u64; BUCKETS.len()],
    sum: f64,
    count: u64,
}

#[derive(Default)]
struct MetricsInner {
    requests: HashMap<(String, String, String), u64>,
    durations: HashMap<(String, String), Histogram>,
}

#[derive(Default)]
pub struct Metrics {
    inner: Mutex<MetricsInner>,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&self, method: &str, status: u16, route: &str, duration: Duration) {
        let status_class = format!("{}xx", status / 100);
        let secs = duration.as_secs_f64();
        let mut inner = self.inner.lock().expect("metrics mutex poisoned");
        *inner
            .requests
            .entry((method.to_string(), status_class, route.to_string()))
            .or_default() += 1;
        let hist = inner
            .durations
            .entry((method.to_string(), route.to_string()))
            .or_default();
        for (idx, bucket) in BUCKETS.iter().enumerate() {
            if secs <= *bucket {
                hist.buckets[idx] += 1;
            }
        }
        hist.sum += secs;
        hist.count += 1;
    }

    pub fn render(&self) -> String {
        let inner = self.inner.lock().expect("metrics mutex poisoned");
        let mut out = String::new();
        out.push_str("# TYPE nanoweb_http_requests_total counter\n");
        for ((method, status_class, route), count) in &inner.requests {
            out.push_str(&format!(
                "nanoweb_http_requests_total{{method=\"{}\",status_class=\"{}\",route=\"{}\"}} {}\n",
                esc(method),
                esc(status_class),
                esc(route),
                count
            ));
        }
        out.push_str("# TYPE nanoweb_http_request_duration_seconds histogram\n");
        for ((method, route), hist) in &inner.durations {
            for (idx, bucket) in BUCKETS.iter().enumerate() {
                out.push_str(&format!(
                    "nanoweb_http_request_duration_seconds_bucket{{method=\"{}\",route=\"{}\",le=\"{}\"}} {}\n",
                    esc(method),
                    esc(route),
                    bucket,
                    hist.buckets[idx]
                ));
            }
            out.push_str(&format!(
                "nanoweb_http_request_duration_seconds_bucket{{method=\"{}\",route=\"{}\",le=\"+Inf\"}} {}\n",
                esc(method),
                esc(route),
                hist.count
            ));
            out.push_str(&format!(
                "nanoweb_http_request_duration_seconds_sum{{method=\"{}\",route=\"{}\"}} {}\n",
                esc(method),
                esc(route),
                hist.sum
            ));
            out.push_str(&format!(
                "nanoweb_http_request_duration_seconds_count{{method=\"{}\",route=\"{}\"}} {}\n",
                esc(method),
                esc(route),
                hist.count
            ));
        }
        out
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', r"\\").replace('"', r#"\""#)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_counter_and_histogram() {
        let metrics = Metrics::new();
        metrics.observe("GET", 200, "/x", Duration::from_millis(3));
        let text = metrics.render();
        assert!(text.contains("nanoweb_http_requests_total"));
        assert!(text.contains("nanoweb_http_request_duration_seconds_bucket"));
    }
}

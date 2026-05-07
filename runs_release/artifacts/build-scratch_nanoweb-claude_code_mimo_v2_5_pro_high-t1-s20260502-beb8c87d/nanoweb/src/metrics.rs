use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub struct Metrics {
    request_counts: Mutex<HashMap<String, f64>>,
    duration_histogram: Mutex<Vec<f64>>,
    duration_sum: Mutex<f64>,
    duration_count: Mutex<u64>,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            request_counts: Mutex::new(HashMap::new()),
            duration_histogram: Mutex::new(Vec::new()),
            duration_sum: Mutex::new(0.0),
            duration_count: Mutex::new(0),
        }
    }

    pub fn record_request(&self, method: &str, status_class: &str, route: &str) {
        let key = format!("method=\"{method}\",status_class=\"{status_class}\",route=\"{route}\"");
        let mut counts = self.request_counts.lock().unwrap();
        *counts.entry(key).or_insert(0.0) += 1.0;
    }

    pub fn record_duration(&self, duration_secs: f64) {
        let mut hist = self.duration_histogram.lock().unwrap();
        hist.push(duration_secs);
        *self.duration_sum.lock().unwrap() += duration_secs;
        *self.duration_count.lock().unwrap() += 1;
    }

    pub fn render(&self) -> String {
        let mut output = String::new();

        // Counter: HTTP requests
        output.push_str("# HELP nanoweb_http_requests_total Total HTTP requests\n");
        output.push_str("# TYPE nanoweb_http_requests_total counter\n");
        let counts = self.request_counts.lock().unwrap();
        for (labels, value) in counts.iter() {
            output.push_str(&format!(
                "nanoweb_http_requests_total{{{labels}}} {value}\n"
            ));
        }

        // Histogram: request duration
        output.push_str("# HELP nanoweb_request_duration_seconds Request duration in seconds\n");
        output.push_str("# TYPE nanoweb_request_duration_seconds histogram\n");
        let sum = *self.duration_sum.lock().unwrap();
        let count = *self.duration_count.lock().unwrap();

        let buckets = [
            0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ];
        let hist = self.duration_histogram.lock().unwrap();
        for &le in &buckets {
            let c = hist.iter().filter(|&&v| v <= le).count();
            output.push_str(&format!(
                "nanoweb_request_duration_seconds_bucket{{le=\"{le}\"}} {c}\n"
            ));
        }
        output.push_str(&format!(
            "nanoweb_request_duration_seconds_bucket{{le=\"+Inf\"}} {count}\n"
        ));
        output.push_str(&format!("nanoweb_request_duration_seconds_sum {sum}\n"));
        output.push_str(&format!("nanoweb_request_duration_seconds_count {count}\n"));

        output
    }

    pub fn timer() -> Instant {
        Instant::now()
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
    fn test_metrics_record_and_render() {
        let metrics = Metrics::new();
        metrics.record_request("GET", "2xx", "/hello");
        metrics.record_request("GET", "2xx", "/hello");
        metrics.record_request("POST", "4xx", "/api/data");
        metrics.record_duration(0.05);
        metrics.record_duration(0.15);

        let output = metrics.render();
        assert!(output.contains("nanoweb_http_requests_total"));
        assert!(output.contains("method=\"GET\""));
        assert!(output.contains("nanoweb_request_duration_seconds_bucket"));
        assert!(output.contains("nanoweb_request_duration_seconds_sum"));
        assert!(output.contains("nanoweb_request_duration_seconds_count 2"));
    }
}

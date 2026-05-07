use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

pub struct Metrics {
    requests_total: Mutex<HashMap<String, AtomicU64>>,
    request_duration: Mutex<Vec<f64>>,
    active_connections: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            requests_total: Mutex::new(HashMap::new()),
            request_duration: Mutex::new(Vec::new()),
            active_connections: AtomicU64::new(0),
        }
    }

    pub fn record_request(&self, method: &str, status_class: &str, route: &str) {
        let key = format!("method=\"{method}\",status=\"{status_class}\",route=\"{route}\"");
        let mut map = self.requests_total.lock().unwrap();
        map.entry(key)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_duration(&self, duration: Duration) {
        let mut durations = self.request_duration.lock().unwrap();
        durations.push(duration.as_secs_f64());
        if durations.len() > 10000 {
            let excess = durations.len() - 10000;
            durations.drain(0..excess);
        }
    }

    pub fn connection_opened(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn connection_closed(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn render_prometheus(&self) -> String {
        let mut output = String::new();

        // Request counter
        output.push_str("# HELP http_requests_total Total HTTP requests\n");
        output.push_str("# TYPE http_requests_total counter\n");
        let map = self.requests_total.lock().unwrap();
        for (labels, counter) in map.iter() {
            output.push_str(&format!(
                "http_requests_total{{{}}} {}\n",
                labels,
                counter.load(Ordering::Relaxed)
            ));
        }

        // Request duration histogram
        output.push_str("# HELP http_request_duration_seconds Request duration in seconds\n");
        output.push_str("# TYPE http_request_duration_seconds histogram\n");
        let durations = self.request_duration.lock().unwrap();
        let buckets = [
            0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ];
        for &bucket in &buckets {
            let count = durations.iter().filter(|&&d| d <= bucket).count();
            output.push_str(&format!(
                "http_request_duration_seconds_bucket{{le=\"{bucket}\"}} {count}\n"
            ));
        }
        let count = durations.len();
        output.push_str(&format!(
            "http_request_duration_seconds_bucket{{le=\"+Inf\"}} {count}\n"
        ));
        let sum: f64 = durations.iter().sum();
        output.push_str(&format!("http_request_duration_seconds_sum {sum:.6}\n"));
        output.push_str(&format!("http_request_duration_seconds_count {count}\n"));

        // Active connections
        output.push_str("# HELP active_connections Current active connections\n");
        output.push_str("# TYPE active_connections gauge\n");
        output.push_str(&format!(
            "active_connections {}\n",
            self.active_connections.load(Ordering::Relaxed)
        ));

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_recording() {
        let metrics = Metrics::new();
        metrics.record_request("GET", "2xx", "/api/test");
        metrics.record_request("GET", "2xx", "/api/test");
        metrics.record_request("POST", "4xx", "/api/test");

        let output = metrics.render_prometheus();
        assert!(output.contains("http_requests_total"));
        assert!(output.contains("GET"));
    }

    #[test]
    fn test_metrics_duration_histogram() {
        let metrics = Metrics::new();
        metrics.record_duration(Duration::from_millis(100));
        metrics.record_duration(Duration::from_millis(200));

        let output = metrics.render_prometheus();
        assert!(output.contains("http_request_duration_seconds"));
    }

    #[test]
    fn test_active_connections() {
        let metrics = Metrics::new();
        metrics.connection_opened();
        metrics.connection_opened();
        assert_eq!(metrics.active_connections.load(Ordering::Relaxed), 2);
        metrics.connection_closed();
        assert_eq!(metrics.active_connections.load(Ordering::Relaxed), 1);
    }
}

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Mutex;
use std::time::Instant;

pub struct MetricsRegistry {
    counters: Mutex<HashMap<String, Counter>>,
    histograms: Mutex<HashMap<String, Histogram>>,
}

struct Counter {
    help: String,
    value: f64,
}

struct Histogram {
    help: String,
    buckets: Vec<f64>,
    counts: Vec<u64>,
    sum: f64,
    count: u64,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        let mut histograms = HashMap::new();
        histograms.insert(
            "http_request_duration_seconds".into(),
            Histogram {
                help: "Request duration in seconds".into(),
                buckets: vec![
                    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
                ],
                counts: vec![0; 13], // 12 buckets + inf
                sum: 0.0,
                count: 0,
            },
        );

        Self {
            counters: Mutex::new(HashMap::new()),
            histograms: Mutex::new(histograms),
        }
    }

    pub fn inc_request_counter(&self, method: &str, status: u16, route: &str) {
        let status_class = format!("{}xx", status / 100);
        let key = format!("http_requests_total{{method=\"{method}\",status_class=\"{status_class}\",route=\"{route}\"}}");
        let mut counters = self.counters.lock().unwrap();
        let counter = counters.entry(key.clone()).or_insert(Counter {
            help: "Total HTTP requests".into(),
            value: 0.0,
        });
        counter.value += 1.0;
    }

    pub fn record_duration(
        &self,
        method: &str,
        status: u16,
        route: &str,
        duration: std::time::Duration,
    ) {
        let secs = duration.as_secs_f64();
        let mut histograms = self.histograms.lock().unwrap();
        if let Some(hist) = histograms.get_mut("http_request_duration_seconds") {
            for (i, &bucket) in hist.buckets.iter().enumerate() {
                if secs <= bucket {
                    hist.counts[i] += 1;
                }
            }
            hist.counts[hist.buckets.len()] += 1; // +Inf
            hist.sum += secs;
            hist.count += 1;
        }

        // Also track per-route
        let status_class = format!("{}xx", status / 100);
        let key = format!("http_request_duration_seconds{{method=\"{method}\",status_class=\"{status_class}\",route=\"{route}\"}}");
        let mut counters = self.counters.lock().unwrap();
        let counter = counters.entry(key.clone()).or_insert(Counter {
            help: "Per-route request duration sum".into(),
            value: 0.0,
        });
        counter.value += secs;
    }

    pub fn render(&self) -> String {
        let mut output = String::new();

        let counters = self.counters.lock().unwrap();
        for (key, counter) in counters.iter() {
            let _ = writeln!(output, "# HELP http_requests_total {}", counter.help);
            let _ = writeln!(output, "# TYPE http_requests_total counter");
            let _ = writeln!(output, "{key} {}", counter.value);
        }

        let histograms = self.histograms.lock().unwrap();
        for (name, hist) in histograms.iter() {
            let _ = writeln!(output, "# HELP {name} {}", hist.help);
            let _ = writeln!(output, "# TYPE {name} histogram");
            let mut cumulative = 0u64;
            for (i, &bucket) in hist.buckets.iter().enumerate() {
                cumulative += hist.counts[i];
                let _ = writeln!(output, "{name}_bucket{{le=\"{bucket}\"}} {cumulative}");
            }
            cumulative += hist.counts[hist.buckets.len()];
            let _ = writeln!(output, "{name}_bucket{{le=\"+Inf\"}} {cumulative}");
            let _ = writeln!(output, "{name}_sum {}", hist.sum);
            let _ = writeln!(output, "{name}_count {}", hist.count);
        }

        output
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RequestTimer {
    start: Instant,
}

impl RequestTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.start.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_increment() {
        let registry = MetricsRegistry::new();
        registry.inc_request_counter("GET", 200, "/test");
        registry.inc_request_counter("GET", 200, "/test");
        let output = registry.render();
        assert!(output.contains("http_requests_total"));
        assert!(output.contains("2"));
    }

    #[test]
    fn test_histogram_recording() {
        let registry = MetricsRegistry::new();
        registry.record_duration("GET", 200, "/test", std::time::Duration::from_millis(50));
        let output = registry.render();
        assert!(output.contains("http_request_duration_seconds_bucket"));
        assert!(output.contains("http_request_duration_seconds_count 1"));
    }
}

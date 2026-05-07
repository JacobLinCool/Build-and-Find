use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Mutex;
use std::time::Instant;

/// Prometheus metrics registry.
pub struct Metrics {
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

impl Metrics {
    pub fn new() -> Self {
        let mut histograms = HashMap::new();

        // Default request duration histogram with standard buckets
        histograms.insert(
            "nanoweb_request_duration_seconds".into(),
            Histogram {
                help: "Request duration in seconds".into(),
                buckets: vec![
                    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
                ],
                counts: vec![0; 13], // 12 buckets + 1 for +Inf
                sum: 0.0,
                count: 0,
            },
        );

        Self {
            counters: Mutex::new(HashMap::new()),
            histograms: Mutex::new(histograms),
        }
    }

    /// Increment a counter by the given value.
    pub fn inc_counter(&self, name: &str, help: &str, value: f64) {
        let mut counters = self.counters.lock().unwrap();
        let counter = counters.entry(name.to_string()).or_insert_with(|| Counter {
            help: help.to_string(),
            value: 0.0,
        });
        counter.value += value;
    }

    /// Record a request with method, status, and route.
    pub fn record_request(&self, method: &str, status: u16, route: &str, duration: f64) {
        let status_class = format!("{}xx", status / 100);

        // Increment request counter
        let counter_name = format!(
            "nanoweb_http_requests_total{{method=\"{}\",status_class=\"{}\",route=\"{}\"}}",
            method, status_class, route
        );
        self.inc_counter(&counter_name, "Total HTTP requests", 1.0);

        // Record duration in histogram
        let mut histograms = self.histograms.lock().unwrap();
        if let Some(hist) = histograms.get_mut("nanoweb_request_duration_seconds") {
            for (i, &bucket) in hist.buckets.iter().enumerate() {
                if duration <= bucket {
                    hist.counts[i] += 1;
                }
            }
            hist.counts[hist.buckets.len()] += 1; // +Inf
            hist.sum += duration;
            hist.count += 1;
        }
    }

    /// Render metrics in Prometheus text exposition format.
    pub fn render(&self) -> String {
        let mut output = String::new();

        // Render counters
        let counters = self.counters.lock().unwrap();
        for (name, counter) in counters.iter() {
            let _ = writeln!(output, "# HELP {} {}", name, counter.help);
            let _ = writeln!(output, "# TYPE {} counter", name);
            let _ = writeln!(output, "{} {}", name, counter.value);
        }

        // Render histograms
        let histograms = self.histograms.lock().unwrap();
        for (name, hist) in histograms.iter() {
            let _ = writeln!(output, "# HELP {} {}", name, hist.help);
            let _ = writeln!(output, "# TYPE {} histogram", name);
            for (i, &bucket) in hist.buckets.iter().enumerate() {
                let _ = writeln!(
                    output,
                    "{}_bucket{{le=\"{}\"}} {}",
                    name, bucket, hist.counts[i]
                );
            }
            let _ = writeln!(
                output,
                "{}_bucket{{le=\"+Inf\"}} {}",
                name,
                hist.counts[hist.buckets.len()]
            );
            let _ = writeln!(output, "{}_sum {}", name, hist.sum);
            let _ = writeln!(output, "{}_count {}", name, hist.count);
        }

        output
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics timer for measuring request duration.
pub struct MetricsTimer {
    start: Instant,
}

impl MetricsTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_increment() {
        let metrics = Metrics::new();
        metrics.inc_counter("test_total", "Test counter", 1.0);
        metrics.inc_counter("test_total", "Test counter", 2.0);

        let output = metrics.render();
        assert!(output.contains("test_total 3"));
    }

    #[test]
    fn test_histogram_recording() {
        let metrics = Metrics::new();
        metrics.record_request("GET", 200, "/test", 0.05);
        metrics.record_request("POST", 201, "/test", 0.15);

        let output = metrics.render();
        assert!(output.contains("nanoweb_request_duration_seconds"));
        assert!(output.contains("nanoweb_request_duration_seconds_count 2"));
    }

    #[test]
    fn test_request_counter() {
        let metrics = Metrics::new();
        metrics.record_request("GET", 200, "/hello", 0.01);

        let output = metrics.render();
        assert!(output.contains("nanoweb_http_requests_total"));
        assert!(output.contains("GET"));
        assert!(output.contains("2xx"));
    }

    #[test]
    fn test_metrics_timer() {
        let timer = MetricsTimer::start();
        // Some work
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(timer.elapsed_secs() >= 0.01);
    }
}

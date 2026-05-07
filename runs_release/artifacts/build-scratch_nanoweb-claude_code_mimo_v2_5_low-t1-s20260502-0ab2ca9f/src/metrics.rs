use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct MetricKey {
    method: String,
    status_class: String,
    route: String,
}

pub struct Metrics {
    request_count: Mutex<HashMap<MetricKey, AtomicU64>>,
    duration_sum: Mutex<HashMap<String, f64>>,
    duration_count: Mutex<HashMap<String, u64>>,
    start_time: Instant,
}

impl Metrics {
    pub fn new() -> Self {
        Metrics {
            request_count: Mutex::new(HashMap::new()),
            duration_sum: Mutex::new(HashMap::new()),
            duration_count: Mutex::new(HashMap::new()),
            start_time: Instant::now(),
        }
    }

    pub fn record_request(&self, method: &str, status: u16, route: &str, duration_secs: f64) {
        let status_class = format!("{}xx", status / 100);
        let key = MetricKey {
            method: method.to_string(),
            status_class,
            route: route.to_string(),
        };

        self.request_count
            .lock()
            .unwrap()
            .entry(key)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);

        self.duration_sum
            .lock()
            .unwrap()
            .entry(route.to_string())
            .and_modify(|v| *v += duration_secs)
            .or_insert(duration_secs);

        self.duration_count
            .lock()
            .unwrap()
            .entry(route.to_string())
            .and_modify(|v| *v += 1)
            .or_insert(1);
    }

    pub fn render(&self) -> String {
        let mut output = String::new();

        output.push_str("# HELP http_requests_total Total number of HTTP requests\n");
        output.push_str("# TYPE http_requests_total counter\n");
        let counts = self.request_count.lock().unwrap();
        for (key, count) in counts.iter() {
            output.push_str(&format!(
                "http_requests_total{{method=\"{}\",status_class=\"{}\",route=\"{}\"}} {}\n",
                key.method,
                key.status_class,
                key.route,
                count.load(Ordering::Relaxed)
            ));
        }
        drop(counts);

        output.push_str("# HELP http_request_duration_seconds Request duration in seconds\n");
        output.push_str("# TYPE http_request_duration_seconds histogram\n");
        let sums = self.duration_sum.lock().unwrap();
        let cnts = self.duration_count.lock().unwrap();
        let mut routes: Vec<String> = sums.keys().cloned().collect();
        routes.sort();
        for route in &routes {
            if let (Some(sum), Some(count)) = (sums.get(route), cnts.get(route)) {
                if *count > 0 {
                    output.push_str(&format!(
                        "http_request_duration_seconds_sum{{route=\"{}\"}} {}\n",
                        route, sum
                    ));
                    output.push_str(&format!(
                        "http_request_duration_seconds_count{{route=\"{}\"}} {}\n",
                        route, count
                    ));
                }
            }
        }
        drop(sums);
        drop(cnts);

        let uptime = self.start_time.elapsed().as_secs();
        output.push_str(&format!("# HELP process_uptime_seconds Process uptime\n"));
        output.push_str(&format!("# TYPE process_uptime_seconds gauge\n"));
        output.push_str(&format!("process_uptime_seconds {}\n", uptime));

        output
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
    fn test_metrics_render() {
        let metrics = Metrics::new();
        metrics.record_request("GET", 200, "/users", 0.05);
        metrics.record_request("POST", 201, "/users", 0.1);
        let output = metrics.render();
        assert!(output.contains("http_requests_total"));
        assert!(output.contains("GET"));
        assert!(output.contains("POST"));
    }
}

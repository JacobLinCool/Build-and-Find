use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub struct Metrics {
    requests_total: Mutex<Vec<(String, String, String, u64)>>,
    duration_sum: AtomicU64,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            requests_total: Mutex::new(Vec::new()),
            duration_sum: AtomicU64::new(0),
        }
    }
}

impl Metrics {
    pub fn inc_request(&self, method: &str, class: &str, route: &str) {
        self.requests_total
            .lock()
            .unwrap()
            .push((method.into(), class.into(), route.into(), 1));
    }
    pub fn observe_duration(&self, secs: f64) {
        self.duration_sum
            .fetch_add((secs * 1_000_000.0) as u64, Ordering::Relaxed);
    }
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(
            "# HELP http_requests_total Total HTTP requests\n# TYPE http_requests_total counter\n",
        );
        for (m, c, r, v) in self.requests_total.lock().unwrap().iter() {
            s.push_str(&format!(
                "http_requests_total{{method=\"{}\",status_class=\"{}\",route=\"{}\"}} {}\n",
                m, c, r, v
            ));
        }
        s.push_str("# TYPE http_request_duration_seconds histogram\n");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders() {
        assert!(Metrics::default().render().contains("http_requests_total"));
    }
}

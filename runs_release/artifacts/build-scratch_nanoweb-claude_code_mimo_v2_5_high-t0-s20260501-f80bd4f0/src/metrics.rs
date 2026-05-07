use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::http::types::{Response, StatusCode};
use std::sync::Arc;

#[derive(Debug)]
pub struct Metrics {
    requests_total: AtomicU64,
    by_method: Mutex<HashMap<String, AtomicU64>>,
    by_status_class: Mutex<HashMap<String, AtomicU64>>,
    by_route: Mutex<HashMap<String, AtomicU64>>,
    request_durations: Mutex<Vec<f64>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            by_method: Mutex::new(HashMap::new()),
            by_status_class: Mutex::new(HashMap::new()),
            by_route: Mutex::new(HashMap::new()),
            request_durations: Mutex::new(Vec::new()),
        }
    }

    pub fn record_request(
        &self,
        method: &str,
        status: StatusCode,
        route: &str,
        duration_secs: f64,
    ) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);

        // By method
        self.by_method
            .lock()
            .unwrap()
            .entry(method.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);

        // By status class
        let class = format!("{}xx", status.0 / 100);
        self.by_status_class
            .lock()
            .unwrap()
            .entry(class)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);

        // By route
        self.by_route
            .lock()
            .unwrap()
            .entry(route.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);

        // Duration
        self.request_durations.lock().unwrap().push(duration_secs);
    }

    pub fn render(&self) -> String {
        let mut out = String::new();

        out.push_str("# HELP http_requests_total Total HTTP requests\n");
        out.push_str("# TYPE http_requests_total counter\n");
        out.push_str(&format!(
            "http_requests_total {}\n",
            self.requests_total.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP http_requests_by_method Requests by method\n");
        out.push_str("# TYPE http_requests_by_method counter\n");
        for (method, count) in self.by_method.lock().unwrap().iter() {
            out.push_str(&format!(
                "http_requests_by_method{{method=\"{}\"}} {}\n",
                method,
                count.load(Ordering::Relaxed)
            ));
        }

        out.push_str("# HELP http_requests_by_status_class Requests by status class\n");
        out.push_str("# TYPE http_requests_by_status_class counter\n");
        for (class, count) in self.by_status_class.lock().unwrap().iter() {
            out.push_str(&format!(
                "http_requests_by_status_class{{status_class=\"{}\"}} {}\n",
                class,
                count.load(Ordering::Relaxed)
            ));
        }

        out.push_str("# HELP http_requests_by_route Requests by route\n");
        out.push_str("# TYPE http_requests_by_route counter\n");
        for (route, count) in self.by_route.lock().unwrap().iter() {
            out.push_str(&format!(
                "http_requests_by_route{{route=\"{}\"}} {}\n",
                route,
                count.load(Ordering::Relaxed)
            ));
        }

        out.push_str("# HELP http_request_duration_seconds Request duration histogram\n");
        out.push_str("# TYPE http_request_duration_seconds histogram\n");
        let durations = self.request_durations.lock().unwrap();
        if !durations.is_empty() {
            let sum: f64 = durations.iter().sum();
            let count = durations.len();
            let min = durations.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = durations.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let avg = sum / count as f64;
            out.push_str(&format!("http_request_duration_seconds_sum {sum}\n"));
            out.push_str(&format!("http_request_duration_seconds_count {count}\n"));
            out.push_str(&format!("http_request_duration_seconds_min {min}\n"));
            out.push_str(&format!("http_request_duration_seconds_max {max}\n"));
            out.push_str(&format!("http_request_duration_seconds_avg {avg}\n"));
        }

        out
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            by_method: Mutex::new(HashMap::new()),
            by_status_class: Mutex::new(HashMap::new()),
            by_route: Mutex::new(HashMap::new()),
            request_durations: Mutex::new(Vec::new()),
        }
    }
}

pub fn metrics_handler(metrics: &Arc<Metrics>, _req: &crate::http::types::Request) -> Response {
    let body = metrics.render();
    let mut resp = Response::ok();
    resp.body = body.into_bytes();
    resp.headers
        .insert("content-type", "text/plain; version=0.0.4; charset=utf-8");
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_render() {
        let metrics = Metrics::new();
        metrics.record_request("GET", StatusCode::OK, "/", 0.1);
        metrics.record_request("POST", StatusCode::OK, "/api", 0.05);
        let output = metrics.render();
        assert!(output.contains("http_requests_total 2"));
        assert!(output.contains("http_requests_by_method{method=\"GET\"} 1"));
    }

    #[test]
    fn metrics_empty() {
        let metrics = Metrics::new();
        let output = metrics.render();
        assert!(output.contains("http_requests_total 0"));
    }
}

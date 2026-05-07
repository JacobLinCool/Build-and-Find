use crate::http::{Method, StatusCode};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Default)]
pub struct Metrics {
    requests: Mutex<HashMap<(String, String, String), u64>>,
    durations: Mutex<Vec<f64>>,
}

impl Metrics {
    pub fn record(&self, method: &Method, status: StatusCode, route: &str, duration: Duration) {
        *self
            .requests
            .lock()
            .unwrap()
            .entry((
                method.to_string(),
                status.class().to_string(),
                route.to_string(),
            ))
            .or_insert(0) += 1;
        self.durations.lock().unwrap().push(duration.as_secs_f64());
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("# TYPE nanoweb_http_requests_total counter\n");
        for ((method, class, route), value) in self.requests.lock().unwrap().iter() {
            out.push_str(&format!(
                "nanoweb_http_requests_total{{method=\"{}\",status_class=\"{}\",route=\"{}\"}} {}\n",
                esc(method),
                esc(class),
                esc(route),
                value
            ));
        }
        out.push_str("# TYPE nanoweb_request_duration_seconds histogram\n");
        let buckets = [
            0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ];
        let durations = self.durations.lock().unwrap();
        for b in buckets {
            let count = durations.iter().filter(|v| **v <= b).count();
            out.push_str(&format!(
                "nanoweb_request_duration_seconds_bucket{{le=\"{b}\"}} {count}\n"
            ));
        }
        out.push_str(&format!(
            "nanoweb_request_duration_seconds_bucket{{le=\"+Inf\"}} {}\n",
            durations.len()
        ));
        out.push_str(&format!(
            "nanoweb_request_duration_seconds_count {}\n",
            durations.len()
        ));
        out.push_str(&format!(
            "nanoweb_request_duration_seconds_sum {}\n",
            durations.iter().sum::<f64>()
        ));
        out
    }
}

fn esc(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders() {
        let m = super::Metrics::default();
        m.record(
            &crate::http::Method::Get,
            crate::http::StatusCode::Ok,
            "/",
            std::time::Duration::from_millis(1),
        );
        assert!(m.render().contains("nanoweb_http_requests_total"));
    }
}

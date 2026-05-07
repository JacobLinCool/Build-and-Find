//! Liveness and readiness state.

use std::sync::atomic::{AtomicBool, Ordering};

/// Process-wide health state. Wrapped in an `Arc` by the server.
pub struct HealthState {
    live: AtomicBool,
    ready: AtomicBool,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            live: AtomicBool::new(true),
            ready: AtomicBool::new(false),
        }
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    pub fn set_live(&self, live: bool) {
        self.live.store(live, Ordering::SeqCst);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }

    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }
}

impl Default for HealthState {
    fn default() -> Self {
        Self::new()
    }
}

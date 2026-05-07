//! Health probe state.

use std::sync::atomic::{AtomicBool, Ordering};

pub struct Health {
    up: AtomicBool,
    ready: AtomicBool,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            up: AtomicBool::new(true),
            ready: AtomicBool::new(false),
        }
    }
}

impl Health {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_ready(&self, v: bool) {
        self.ready.store(v, Ordering::SeqCst);
    }
    pub fn set_up(&self, v: bool) {
        self.up.store(v, Ordering::SeqCst);
    }
    pub fn is_up(&self) -> bool {
        self.up.load(Ordering::SeqCst)
    }
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_toggles() {
        let h = Health::new();
        assert!(h.is_up());
        assert!(!h.is_ready());
        h.set_ready(true);
        assert!(h.is_ready());
    }
}

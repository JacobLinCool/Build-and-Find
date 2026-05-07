use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub struct Health {
    ready: AtomicBool,
}

impl Health {
    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    pub fn ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn readiness_flips() {
        let h = super::Health::default();
        assert!(!h.ready());
        h.set_ready(true);
        assert!(h.ready());
    }
}

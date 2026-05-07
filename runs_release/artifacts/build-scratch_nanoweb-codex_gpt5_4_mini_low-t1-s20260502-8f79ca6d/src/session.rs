use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;

pub trait SessionStore: Send + Sync {
    fn get(&self, sid: &str) -> Option<HashMap<String, String>>;
    fn set(&self, sid: String, data: HashMap<String, String>);
}

pub struct InMemorySessionStore(Mutex<HashMap<String, HashMap<String, String>>>);
impl InMemorySessionStore {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}
impl SessionStore for InMemorySessionStore {
    fn get(&self, sid: &str) -> Option<HashMap<String, String>> {
        self.0.lock().unwrap().get(sid).cloned()
    }
    fn set(&self, sid: String, data: HashMap<String, String>) {
        self.0.lock().unwrap().insert(sid, data);
    }
}

pub fn sign(secret: &[u8], value: &str) -> String {
    let mut mac: Hmac<Sha256> = Hmac::new_from_slice(secret).unwrap();
    mac.update(value.as_bytes());
    format!(
        "{}:{}",
        value,
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stores_and_signs() {
        let store = InMemorySessionStore::new();
        store.set(
            "a".into(),
            HashMap::from([(String::from("k"), String::from("v"))]),
        );
        assert_eq!(store.get("a").unwrap().get("k").unwrap(), "v");
        assert!(sign(b"secret", "value").starts_with("value:"));
    }
}

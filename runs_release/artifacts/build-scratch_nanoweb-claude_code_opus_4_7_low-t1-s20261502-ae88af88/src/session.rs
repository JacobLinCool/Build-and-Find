use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use parking_lot::Mutex;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;

type HmacSha256 = Hmac<Sha256>;

#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    async fn load(&self, id: &str) -> Option<HashMap<String, String>>;
    async fn save(&self, id: &str, data: HashMap<String, String>);
    async fn destroy(&self, id: &str);
}

pub struct InMemoryStore {
    map: Mutex<HashMap<String, HashMap<String, String>>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionStore for InMemoryStore {
    async fn load(&self, id: &str) -> Option<HashMap<String, String>> {
        self.map.lock().get(id).cloned()
    }
    async fn save(&self, id: &str, data: HashMap<String, String>) {
        self.map.lock().insert(id.to_string(), data);
    }
    async fn destroy(&self, id: &str) {
        self.map.lock().remove(id);
    }
}

pub struct SessionManager {
    pub store: Arc<dyn SessionStore>,
    pub secret: Vec<u8>,
    pub cookie_name: String,
}

impl SessionManager {
    pub fn new(store: Arc<dyn SessionStore>, secret: impl Into<Vec<u8>>) -> Self {
        Self {
            store,
            secret: secret.into(),
            cookie_name: "nanoweb_sid".into(),
        }
    }

    pub fn sign(&self, id: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("hmac key");
        mac.update(id.as_bytes());
        let tag = mac.finalize().into_bytes();
        let tag_b64 = URL_SAFE_NO_PAD.encode(tag);
        format!("{}.{}", id, tag_b64)
    }

    pub fn verify<'a>(&self, signed: &'a str) -> Option<&'a str> {
        let (id, tag) = signed.rsplit_once('.')?;
        let mut mac = HmacSha256::new_from_slice(&self.secret).ok()?;
        mac.update(id.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        if constant_time_eq(expected.as_bytes(), tag.as_bytes()) {
            Some(id)
        } else {
            None
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify() {
        let store = Arc::new(InMemoryStore::new());
        let m = SessionManager::new(store, b"secret".to_vec());
        let signed = m.sign("abc");
        assert_eq!(m.verify(&signed), Some("abc"));
        assert!(m.verify("abc.bad").is_none());
    }
}

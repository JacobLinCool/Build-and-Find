use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub trait SessionStore: Send + Sync {
    fn get(&self, id: &str) -> Option<HashMap<String, Value>>;
    fn set(&self, id: String, data: HashMap<String, Value>);
    fn delete(&self, id: &str);
}

#[derive(Default)]
pub struct MemorySessionStore {
    inner: Mutex<HashMap<String, HashMap<String, Value>>>,
}

impl SessionStore for MemorySessionStore {
    fn get(&self, id: &str) -> Option<HashMap<String, Value>> {
        self.inner.lock().unwrap().get(id).cloned()
    }
    fn set(&self, id: String, data: HashMap<String, Value>) {
        self.inner.lock().unwrap().insert(id, data);
    }
    fn delete(&self, id: &str) {
        self.inner.lock().unwrap().remove(id);
    }
}

#[derive(Clone)]
pub struct Sessions {
    store: Arc<dyn SessionStore>,
    secret: Vec<u8>,
}

impl Sessions {
    pub fn memory(secret: impl Into<Vec<u8>>) -> Self {
        Self {
            store: Arc::new(MemorySessionStore::default()),
            secret: secret.into(),
        }
    }

    pub fn sign(&self, id: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("hmac accepts any key");
        mac.update(id.as_bytes());
        format!(
            "{id}.{}",
            base64::Engine::encode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                mac.finalize().into_bytes()
            )
        )
    }

    pub fn verify(&self, cookie: &str) -> Option<String> {
        let (id, _) = cookie.rsplit_once('.')?;
        (self.sign(id) == cookie).then(|| id.to_string())
    }

    pub fn store(&self) -> Arc<dyn SessionStore> {
        self.store.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_and_verifies() {
        let s = Sessions::memory("secret");
        let c = s.sign("abc");
        assert_eq!(s.verify(&c).as_deref(), Some("abc"));
    }
}

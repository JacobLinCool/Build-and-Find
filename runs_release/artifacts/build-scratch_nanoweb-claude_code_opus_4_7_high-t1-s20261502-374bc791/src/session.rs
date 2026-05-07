//! Session storage with HMAC-signed session-id cookies.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::{NanowebError, Result};

pub type SessionData = HashMap<String, String>;

/// Pluggable session storage.
pub trait SessionStore: Send + Sync {
    fn load(&self, id: &str) -> Option<SessionData>;
    fn save(&self, id: &str, data: SessionData);
    fn delete(&self, id: &str);
}

#[derive(Default)]
pub struct InMemoryStore {
    inner: Mutex<HashMap<String, SessionData>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SessionStore for InMemoryStore {
    fn load(&self, id: &str) -> Option<SessionData> {
        let g = self.inner.lock().expect("session store lock");
        g.get(id).cloned()
    }
    fn save(&self, id: &str, data: SessionData) {
        let mut g = self.inner.lock().expect("session store lock");
        g.insert(id.to_string(), data);
    }
    fn delete(&self, id: &str) {
        let mut g = self.inner.lock().expect("session store lock");
        g.remove(id);
    }
}

/// Manager that signs and verifies session-id cookies with HMAC-SHA256.
pub struct SessionManager {
    pub store: Arc<dyn SessionStore>,
    pub cookie_name: String,
    secret: Vec<u8>,
}

impl SessionManager {
    pub fn new(secret: impl Into<Vec<u8>>, store: Arc<dyn SessionStore>) -> Self {
        Self {
            store,
            cookie_name: "session".into(),
            secret: secret.into(),
        }
    }

    pub fn sign(&self, id: &str) -> String {
        let mut mac = <Hmac<Sha256>>::new_from_slice(&self.secret).expect("hmac");
        mac.update(id.as_bytes());
        let tag = mac.finalize().into_bytes();
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(tag);
        format!("{}.{}", id, b64)
    }

    pub fn verify(&self, signed: &str) -> Result<String> {
        let (id, sig) = signed
            .rsplit_once('.')
            .ok_or_else(|| NanowebError::Unauthorized("malformed session cookie".into()))?;
        let expected = self.sign(id);
        let provided = signed;
        if !constant_time_eq(expected.as_bytes(), provided.as_bytes()) {
            let _ = sig;
            return Err(NanowebError::Unauthorized(
                "invalid session signature".into(),
            ));
        }
        Ok(id.to_string())
    }

    pub fn new_id() -> String {
        let id = uuid::Uuid::new_v4();
        let bytes = id.as_bytes();
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_and_verifies() {
        let store: Arc<dyn SessionStore> = Arc::new(InMemoryStore::new());
        let mgr = SessionManager::new(b"secret".to_vec(), store);
        let id = SessionManager::new_id();
        let signed = mgr.sign(&id);
        assert_eq!(mgr.verify(&signed).unwrap(), id);
    }

    #[test]
    fn rejects_tampering() {
        let store: Arc<dyn SessionStore> = Arc::new(InMemoryStore::new());
        let mgr = SessionManager::new(b"secret".to_vec(), store);
        let id = SessionManager::new_id();
        let mut signed = mgr.sign(&id);
        signed.push('x');
        assert!(mgr.verify(&signed).is_err());
    }

    #[test]
    fn store_round_trip() {
        let store = InMemoryStore::new();
        let mut data = SessionData::new();
        data.insert("user".into(), "alice".into());
        store.save("abc", data.clone());
        assert_eq!(store.load("abc").unwrap().get("user").unwrap(), "alice");
        store.delete("abc");
        assert!(store.load("abc").is_none());
    }
}

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::error::{NanowebError, Result};

type HmacSha256 = Hmac<Sha256>;
pub type SessionData = BTreeMap<String, String>;

pub trait SessionStore: Send + Sync + 'static {
    fn load(&self, id: &str) -> Option<SessionData>;
    fn save(&self, id: &str, data: SessionData);
    fn delete(&self, id: &str);
}

#[derive(Clone, Default)]
pub struct MemorySessionStore {
    inner: Arc<Mutex<BTreeMap<String, SessionData>>>,
}

impl SessionStore for MemorySessionStore {
    fn load(&self, id: &str) -> Option<SessionData> {
        self.inner
            .lock()
            .expect("session mutex poisoned")
            .get(id)
            .cloned()
    }

    fn save(&self, id: &str, data: SessionData) {
        self.inner
            .lock()
            .expect("session mutex poisoned")
            .insert(id.to_string(), data);
    }

    fn delete(&self, id: &str) {
        self.inner
            .lock()
            .expect("session mutex poisoned")
            .remove(id);
    }
}

#[derive(Clone)]
pub struct SessionManager<S: SessionStore = MemorySessionStore> {
    store: S,
    secret: Arc<Vec<u8>>,
}

impl<S: SessionStore> SessionManager<S> {
    pub fn new(store: S, secret: impl AsRef<[u8]>) -> Self {
        Self {
            store,
            secret: Arc::new(secret.as_ref().to_vec()),
        }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn new_session_id(&self) -> String {
        let mut bytes = [0_u8; 24];
        rand::thread_rng().fill_bytes(&mut bytes);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    pub fn sign_cookie(&self, session_id: &str) -> Result<String> {
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .map_err(|_| NanowebError::Config("invalid session secret".into()))?;
        mac.update(session_id.as_bytes());
        let sig =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        Ok(format!("{session_id}.{sig}"))
    }

    pub fn verify_cookie(&self, cookie: &str) -> Result<Option<String>> {
        let Some((id, sig)) = cookie.rsplit_once('.') else {
            return Ok(None);
        };
        let expected = self.sign_cookie(id)?;
        let expected_sig = expected.rsplit_once('.').map(|(_, s)| s).unwrap_or("");
        let valid = bool::from(sig.as_bytes().ct_eq(expected_sig.as_bytes()));
        Ok(valid.then(|| id.to_string()))
    }
}

impl Default for SessionManager<MemorySessionStore> {
    fn default() -> Self {
        Self::new(MemorySessionStore::default(), "development-secret")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_and_verifies_cookie() {
        let manager = SessionManager::new(MemorySessionStore::default(), "secret");
        let signed = manager.sign_cookie("abc").unwrap();
        assert_eq!(
            manager.verify_cookie(&signed).unwrap().as_deref(),
            Some("abc")
        );
        assert!(manager.verify_cookie("abc.bad").unwrap().is_none());
    }

    #[test]
    fn memory_store_roundtrip() {
        let store = MemorySessionStore::default();
        let mut data = SessionData::new();
        data.insert("user".into(), "ada".into());
        store.save("id", data.clone());
        assert_eq!(store.load("id"), Some(data));
    }
}

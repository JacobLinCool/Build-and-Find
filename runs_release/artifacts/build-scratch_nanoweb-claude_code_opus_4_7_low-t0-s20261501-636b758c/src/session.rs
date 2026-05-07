//! Session cookies with HMAC-SHA256-signed identifiers and a pluggable store.

use crate::NanowebError;
use async_trait::async_trait;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;

#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    async fn get(&self, id: &str) -> Option<HashMap<String, String>>;
    async fn set(&self, id: &str, data: HashMap<String, String>);
    async fn destroy(&self, id: &str);
}

pub struct InMemoryStore {
    inner: Mutex<HashMap<String, HashMap<String, String>>>,
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl SessionStore for InMemoryStore {
    async fn get(&self, id: &str) -> Option<HashMap<String, String>> {
        self.inner.lock().unwrap().get(id).cloned()
    }
    async fn set(&self, id: &str, data: HashMap<String, String>) {
        self.inner.lock().unwrap().insert(id.to_string(), data);
    }
    async fn destroy(&self, id: &str) {
        self.inner.lock().unwrap().remove(id);
    }
}

type HmacSha256 = Hmac<Sha256>;

pub fn sign(secret: &[u8], value: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac key");
    mac.update(value.as_bytes());
    let tag = mac.finalize().into_bytes();
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(tag);
    format!("{}.{}", value, b64)
}

pub fn verify(secret: &[u8], signed: &str) -> Result<String, NanowebError> {
    let (value, sig) = signed
        .rsplit_once('.')
        .ok_or_else(|| NanowebError::Session("malformed signed cookie".into()))?;
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac key");
    mac.update(value.as_bytes());
    let tag = mac.finalize().into_bytes();
    let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(tag);
    if expected == sig {
        Ok(value.to_string())
    } else {
        Err(NanowebError::Session("invalid signature".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_round_trip() {
        let s = sign(b"secret", "abc123");
        let v = verify(b"secret", &s).unwrap();
        assert_eq!(v, "abc123");
    }

    #[test]
    fn rejects_bad_sig() {
        let mut s = sign(b"secret", "abc123");
        s.push('x');
        assert!(verify(b"secret", &s).is_err());
    }

    #[tokio::test]
    async fn in_memory_store() {
        let s = InMemoryStore::default();
        let mut data = HashMap::new();
        data.insert("k".into(), "v".into());
        s.set("id", data.clone()).await;
        assert_eq!(s.get("id").await, Some(data));
        s.destroy("id").await;
        assert!(s.get("id").await.is_none());
    }
}

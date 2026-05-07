use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use parking_lot::Mutex;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;

type HmacSha256 = Hmac<Sha256>;

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn get(&self, sid: &str) -> Option<HashMap<String, String>>;
    async fn put(&self, sid: &str, data: HashMap<String, String>);
    async fn delete(&self, sid: &str);
}

#[derive(Default, Clone)]
pub struct MemoryStore {
    inner: Arc<Mutex<HashMap<String, HashMap<String, String>>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SessionStore for MemoryStore {
    async fn get(&self, sid: &str) -> Option<HashMap<String, String>> {
        self.inner.lock().get(sid).cloned()
    }
    async fn put(&self, sid: &str, data: HashMap<String, String>) {
        self.inner.lock().insert(sid.to_string(), data);
    }
    async fn delete(&self, sid: &str) {
        self.inner.lock().remove(sid);
    }
}

/// Sign a session id with HMAC-SHA256, returning `id.b64sig`.
pub fn sign(secret: &[u8], id: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac key length");
    mac.update(id.as_bytes());
    let sig = mac.finalize().into_bytes();
    format!("{}.{}", id, URL_SAFE_NO_PAD.encode(sig))
}

/// Verify a signed session cookie value, returning the bare session id.
pub fn verify(secret: &[u8], signed: &str) -> Option<String> {
    let (id, sig_b64) = signed.rsplit_once('.')?;
    let sig = URL_SAFE_NO_PAD.decode(sig_b64).ok()?;
    let mut mac = HmacSha256::new_from_slice(secret).ok()?;
    mac.update(id.as_bytes());
    mac.verify_slice(&sig).ok()?;
    Some(id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_then_verify() {
        let secret = b"abcdefg";
        let signed = sign(secret, "sess-1");
        assert_eq!(verify(secret, &signed).as_deref(), Some("sess-1"));
    }

    #[test]
    fn tampered_rejected() {
        let signed = sign(b"key1", "sess-1");
        let mut bad = signed.clone();
        bad.push('x');
        assert!(verify(b"key1", &bad).is_none());
    }

    #[tokio::test]
    async fn memory_store_roundtrip() {
        let s = MemoryStore::new();
        let mut data = HashMap::new();
        data.insert("user".into(), "alice".into());
        s.put("id1", data.clone()).await;
        assert_eq!(s.get("id1").await, Some(data));
        s.delete("id1").await;
        assert!(s.get("id1").await.is_none());
    }
}

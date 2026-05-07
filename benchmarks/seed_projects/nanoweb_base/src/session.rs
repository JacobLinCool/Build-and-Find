//! Session cookies signed with HMAC-SHA256.
//!
//! The default `SessionStore` implementation keeps everything in-memory.
//! Applications can provide their own store by implementing the trait.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::NanowebError;

type HmacSha256 = Hmac<Sha256>;

pub const DEFAULT_COOKIE_NAME: &str = "nano_session";

/// The object-safe store trait.
pub trait SessionStore: Send + Sync {
    fn get(&self, id: &str) -> Option<String>;
    fn set(&self, id: &str, value: String);
    fn remove(&self, id: &str);
}

/// In-memory store with a best-effort TTL sweeper.
pub struct InMemoryStore {
    ttl: Duration,
    inner: Mutex<HashMap<String, (Instant, String)>>,
}

impl InMemoryStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn sweep(&self, guard: &mut HashMap<String, (Instant, String)>) {
        let ttl = self.ttl;
        guard.retain(|_, (at, _)| at.elapsed() <= ttl);
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new(Duration::from_secs(60 * 60))
    }
}

impl SessionStore for InMemoryStore {
    fn get(&self, id: &str) -> Option<String> {
        let mut g = self.inner.lock().ok()?;
        self.sweep(&mut g);
        g.get(id).map(|(_, v)| v.clone())
    }

    fn set(&self, id: &str, value: String) {
        if let Ok(mut g) = self.inner.lock() {
            self.sweep(&mut g);
            g.insert(id.to_string(), (Instant::now(), value));
        }
    }

    fn remove(&self, id: &str) {
        if let Ok(mut g) = self.inner.lock() {
            g.remove(id);
        }
    }
}

/// Sign `payload` using the secret key, returning `payload.signature`.
pub fn sign(secret: &[u8], payload: &str) -> Result<String, NanowebError> {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(secret)
        .map_err(|e| NanowebError::Session(format!("hmac key: {}", e)))?;
    Mac::update(&mut mac, payload.as_bytes());
    let sig = mac.finalize().into_bytes();
    Ok(format!("{}.{}", payload, hex(&sig)))
}

/// Verify the signature attached to `signed` and return the payload on
/// success.
pub fn verify(secret: &[u8], signed: &str) -> Result<String, NanowebError> {
    let (payload, sig) = signed
        .rsplit_once('.')
        .ok_or_else(|| NanowebError::Session("missing signature".into()))?;
    let mut mac = <HmacSha256 as Mac>::new_from_slice(secret)
        .map_err(|e| NanowebError::Session(format!("hmac key: {}", e)))?;
    Mac::update(&mut mac, payload.as_bytes());
    let expected = mac.finalize().into_bytes();
    let expected_hex = hex(&expected);
    if !constant_time_eq(expected_hex.as_bytes(), sig.as_bytes()) {
        return Err(NanowebError::Session("bad signature".into()));
    }
    Ok(payload.to_string())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
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
    fn sign_then_verify() {
        let signed = sign(b"secret", "user=alice").unwrap();
        let payload = verify(b"secret", &signed).unwrap();
        assert_eq!(payload, "user=alice");
    }

    #[test]
    fn verify_rejects_tamper() {
        let signed = sign(b"secret", "user=alice").unwrap();
        let tampered = signed.replace("alice", "mallory");
        assert!(verify(b"secret", &tampered).is_err());
    }

    #[test]
    fn in_memory_round_trip() {
        let store = InMemoryStore::new(Duration::from_secs(60));
        store.set("sid", "hello".into());
        assert_eq!(store.get("sid").as_deref(), Some("hello"));
        store.remove("sid");
        assert!(store.get("sid").is_none());
    }
}

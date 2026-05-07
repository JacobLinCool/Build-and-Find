use async_trait::async_trait;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;

type HmacSha256 = Hmac<Sha256>;

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn get(&self, session_id: &str) -> Option<HashMap<String, String>>;
    async fn set(&self, session_id: &str, data: HashMap<String, String>);
    async fn remove(&self, session_id: &str);
}

pub struct InMemorySessionStore {
    sessions: Mutex<HashMap<String, HashMap<String, String>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn get(&self, session_id: &str) -> Option<HashMap<String, String>> {
        self.sessions.lock().unwrap().get(session_id).cloned()
    }

    async fn set(&self, session_id: &str, data: HashMap<String, String>) {
        self.sessions
            .lock()
            .unwrap()
            .insert(session_id.to_string(), data);
    }

    async fn remove(&self, session_id: &str) {
        self.sessions.lock().unwrap().remove(session_id);
    }
}

pub struct SessionManager {
    store: Box<dyn SessionStore>,
    secret: Vec<u8>,
    cookie_name: String,
}

impl SessionManager {
    pub fn new(store: Box<dyn SessionStore>, secret: &[u8]) -> Self {
        Self {
            store,
            secret: secret.to_vec(),
            cookie_name: "session_id".into(),
        }
    }

    pub fn cookie_name(&self) -> &str {
        &self.cookie_name
    }

    pub fn sign_session_id(&self, session_id: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        mac.update(session_id.as_bytes());
        let signature = mac.finalize().into_bytes();
        let sig_hex = hex_encode(&signature);
        format!("{session_id}.{sig_hex}")
    }

    pub fn verify_session_cookie(&self, cookie: &str) -> Option<String> {
        let dot = cookie.rfind('.')?;
        let session_id = &cookie[..dot];
        let sig_hex = &cookie[dot + 1..];

        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        mac.update(session_id.as_bytes());
        let expected = mac.finalize().into_bytes();
        let expected_hex = hex_encode(&expected);

        if constant_time_eq(expected_hex.as_bytes(), sig_hex.as_bytes()) {
            Some(session_id.to_string())
        } else {
            None
        }
    }

    pub fn extract_session_cookie(&self, cookie_header: &str) -> Option<String> {
        for part in cookie_header.split(';') {
            let part = part.trim();
            if let Some(value) = part.strip_prefix(&format!("{}=", self.cookie_name)) {
                return self.verify_session_cookie(value);
            }
        }
        None
    }

    pub async fn get_session(&self, session_id: &str) -> Option<HashMap<String, String>> {
        self.store.get(session_id).await
    }

    pub async fn set_session(&self, session_id: &str, data: HashMap<String, String>) {
        self.store.set(session_id, data).await;
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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

    #[tokio::test]
    async fn test_in_memory_session() {
        let store = InMemorySessionStore::new();
        let mut data = HashMap::new();
        data.insert("user".into(), "alice".into());
        store.set("abc", data.clone()).await;
        let retrieved = store.get("abc").await.unwrap();
        assert_eq!(retrieved.get("user").map(|s| s.as_str()), Some("alice"));
        store.remove("abc").await;
        assert!(store.get("abc").await.is_none());
    }

    #[test]
    fn test_session_signing() {
        let manager = SessionManager::new(Box::new(InMemorySessionStore::new()), b"test-secret");
        let signed = manager.sign_session_id("session123");
        assert!(signed.starts_with("session123."));
        let verified = manager.verify_session_cookie(&signed);
        assert_eq!(verified.as_deref(), Some("session123"));
    }

    #[test]
    fn test_bad_signature() {
        let manager = SessionManager::new(Box::new(InMemorySessionStore::new()), b"test-secret");
        let result = manager.verify_session_cookie("session123.badsignature");
        assert!(result.is_none());
    }
}

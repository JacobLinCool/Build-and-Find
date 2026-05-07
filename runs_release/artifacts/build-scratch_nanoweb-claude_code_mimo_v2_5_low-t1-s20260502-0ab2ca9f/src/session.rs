use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub trait SessionStore: Send + Sync {
    fn get(&self, session_id: &str) -> Option<HashMap<String, String>>;
    fn set(&self, session_id: &str, data: HashMap<String, String>);
    fn delete(&self, session_id: &str);
}

pub struct InMemorySessionStore {
    sessions: Mutex<HashMap<String, HashMap<String, String>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        InMemorySessionStore {
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionStore for InMemorySessionStore {
    fn get(&self, session_id: &str) -> Option<HashMap<String, String>> {
        self.sessions.lock().unwrap().get(session_id).cloned()
    }

    fn set(&self, session_id: &str, data: HashMap<String, String>) {
        self.sessions
            .lock()
            .unwrap()
            .insert(session_id.to_string(), data);
    }

    fn delete(&self, session_id: &str) {
        self.sessions.lock().unwrap().remove(session_id);
    }
}

pub struct SessionManager {
    secret: Vec<u8>,
    store: Arc<dyn SessionStore>,
}

impl SessionManager {
    pub fn new(secret: &str, store: Arc<dyn SessionStore>) -> Self {
        SessionManager {
            secret: secret.as_bytes().to_vec(),
            store,
        }
    }

    pub fn sign(&self, session_id: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key size");
        mac.update(session_id.as_bytes());
        let result = mac.finalize();
        let sig = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            &result.into_bytes(),
        );
        format!("{}.{}", session_id, sig)
    }

    pub fn verify(&self, signed: &str) -> Option<String> {
        let parts: Vec<&str> = signed.splitn(2, '.').collect();
        if parts.len() != 2 {
            return None;
        }
        let session_id = parts[0];
        let sig_b64 = parts[1];

        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key size");
        mac.update(session_id.as_bytes());

        let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(sig_b64)
            .ok()?;

        if mac.verify_slice(&sig_bytes).is_ok() {
            Some(session_id.to_string())
        } else {
            None
        }
    }

    pub fn get_session(&self, signed_id: &str) -> Option<HashMap<String, String>> {
        let session_id = self.verify(signed_id)?;
        self.store.get(&session_id)
    }

    pub fn set_session(&self, session_id: &str, data: HashMap<String, String>) {
        self.store.set(session_id, data);
    }

    pub fn extract_from_cookies(&self, cookie_header: &str) -> Option<String> {
        cookie_header.split(';').find_map(|pair| {
            let pair = pair.trim();
            pair.strip_prefix("session=").map(|v| v.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify() {
        let store = Arc::new(InMemorySessionStore::new());
        let manager = SessionManager::new("test-secret-key", store);
        let signed = manager.sign("abc123");
        let verified = manager.verify(&signed);
        assert_eq!(verified, Some("abc123".to_string()));
    }

    #[test]
    fn test_verify_invalid() {
        let store = Arc::new(InMemorySessionStore::new());
        let manager = SessionManager::new("test-secret-key", store);
        let result = manager.verify("invalid.token");
        assert!(result.is_none());
    }

    #[test]
    fn test_session_store() {
        let store = Arc::new(InMemorySessionStore::new());
        let manager = SessionManager::new("test-secret", store.clone());

        let mut data = HashMap::new();
        data.insert("user".into(), "alice".into());
        manager.set_session("sess1", data);

        let signed = manager.sign("sess1");
        let session = manager.get_session(&signed).unwrap();
        assert_eq!(session.get("user"), Some(&"alice".to_string()));
    }
}

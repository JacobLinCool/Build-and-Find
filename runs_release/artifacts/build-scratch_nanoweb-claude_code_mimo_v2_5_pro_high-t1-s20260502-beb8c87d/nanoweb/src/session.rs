use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::RwLock;

type HmacSha256 = Hmac<Sha256>;

pub trait SessionStore: Send + Sync {
    fn get(&self, session_id: &str) -> Option<HashMap<String, String>>;
    fn set(&self, session_id: &str, data: HashMap<String, String>);
    fn remove(&self, session_id: &str);
}

pub struct InMemorySessionStore {
    sessions: RwLock<HashMap<String, HashMap<String, String>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
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
        self.sessions.read().ok()?.get(session_id).cloned()
    }

    fn set(&self, session_id: &str, data: HashMap<String, String>) {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.insert(session_id.to_string(), data);
        }
    }

    fn remove(&self, session_id: &str) {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.remove(session_id);
        }
    }
}

pub struct SessionManager {
    secret: Vec<u8>,
    cookie_name: String,
    pub store: Box<dyn SessionStore>,
}

impl SessionManager {
    pub fn new(secret: &str, store: Box<dyn SessionStore>) -> Self {
        Self {
            secret: secret.as_bytes().to_vec(),
            cookie_name: "nanoweb_session".to_string(),
            store,
        }
    }

    pub fn create_session(&self) -> (String, String) {
        let session_id = uuid::Uuid::new_v4().to_string();
        let signed = self.sign(&session_id);
        self.store.set(&session_id, HashMap::new());
        (session_id, signed)
    }

    pub fn get_session_id_from_cookie(&self, cookie_header: &str) -> Option<String> {
        for cookie in cookie_header.split(';') {
            let cookie = cookie.trim();
            if let Some(value) = cookie.strip_prefix(&format!("{}=", self.cookie_name)) {
                if let Some(session_id) = self.verify(value) {
                    return Some(session_id);
                }
            }
        }
        None
    }

    pub fn cookie_name(&self) -> &str {
        &self.cookie_name
    }

    pub fn sign(&self, data: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key size");
        mac.update(data.as_bytes());
        let sig = mac.finalize().into_bytes();
        let sig_b64 = URL_SAFE_NO_PAD.encode(&sig);
        format!("{data}.{sig_b64}")
    }

    pub fn verify(&self, signed: &str) -> Option<String> {
        let dot_pos = signed.rfind('.')?;
        let data = &signed[..dot_pos];
        let sig_b64 = &signed[dot_pos + 1..];

        let sig = URL_SAFE_NO_PAD.decode(sig_b64).ok()?;
        let mut mac = HmacSha256::new_from_slice(&self.secret).ok()?;
        mac.update(data.as_bytes());
        mac.verify_slice(&sig).ok()?;

        Some(data.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_create_and_verify() {
        let store = InMemorySessionStore::new();
        let manager = SessionManager::new("test-secret", Box::new(store));
        let (session_id, signed) = manager.create_session();
        let verified = manager.verify(&signed).unwrap();
        assert_eq!(verified, session_id);
    }

    #[test]
    fn test_session_tamper_detection() {
        let store = InMemorySessionStore::new();
        let manager = SessionManager::new("test-secret", Box::new(store));
        let (_, signed) = manager.create_session();
        let tampered = format!("tampered{}", &signed[signed.find('.').unwrap()..]);
        assert!(manager.verify(&tampered).is_none());
    }

    #[test]
    fn test_in_memory_store() {
        let store = InMemorySessionStore::new();
        let mut data = HashMap::new();
        data.insert("user".to_string(), "alice".to_string());
        store.set("s1", data);
        let retrieved = store.get("s1").unwrap();
        assert_eq!(retrieved.get("user").unwrap(), "alice");
        store.remove("s1");
        assert!(store.get("s1").is_none());
    }

    #[test]
    fn test_get_session_from_cookie() {
        let store = InMemorySessionStore::new();
        let manager = SessionManager::new("test-secret", Box::new(store));
        let (session_id, signed) = manager.create_session();
        let cookie = format!("nanoweb_session={signed}; other=value");
        let recovered = manager.get_session_id_from_cookie(&cookie).unwrap();
        assert_eq!(recovered, session_id);
    }
}

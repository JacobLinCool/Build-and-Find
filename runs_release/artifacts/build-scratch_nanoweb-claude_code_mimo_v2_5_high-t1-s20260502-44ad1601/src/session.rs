use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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

#[derive(Clone)]
pub struct SessionManager {
    store: Arc<dyn SessionStore>,
    secret: Arc<Vec<u8>>,
    cookie_name: String,
}

impl SessionManager {
    pub fn new(store: Arc<dyn SessionStore>, secret: &str) -> Self {
        SessionManager {
            store,
            secret: Arc::new(secret.as_bytes().to_vec()),
            cookie_name: "session_id".into(),
        }
    }

    pub fn sign(&self, data: &str) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key size");
        mac.update(data.as_bytes());
        let result = mac.finalize().into_bytes();
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD_NO_PAD, &result)
    }

    pub fn verify(&self, data: &str, signature: &str) -> bool {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key size");
        mac.update(data.as_bytes());

        if let Ok(decoded) =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD_NO_PAD, signature)
        {
            mac.verify_slice(&decoded).is_ok()
        } else {
            false
        }
    }

    pub fn get_session_id_from_cookie(&self, cookie_header: Option<&str>) -> Option<String> {
        cookie_header.and_then(|cookies| {
            cookies.split(';').find_map(|c| {
                let c = c.trim();
                if let Some((k, v)) = c.split_once('=') {
                    if k.trim() == self.cookie_name {
                        return Some(v.trim().to_string());
                    }
                }
                None
            })
        })
    }

    pub fn get_session(&self, session_id: &str) -> HashMap<String, String> {
        self.store.get(session_id).unwrap_or_default()
    }

    pub fn set_session(&self, session_id: &str, data: HashMap<String, String>) {
        self.store.set(session_id, data);
    }

    pub fn cookie_name(&self) -> &str {
        &self.cookie_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_store() {
        let store = InMemorySessionStore::new();
        let mut data = HashMap::new();
        data.insert("user".into(), "alice".into());
        store.set("sid1", data.clone());
        assert_eq!(store.get("sid1").unwrap(), data);
        store.delete("sid1");
        assert!(store.get("sid1").is_none());
    }

    #[test]
    fn test_hmac_sign_verify() {
        let manager = SessionManager::new(Arc::new(InMemorySessionStore::new()), "test-secret");
        let sig = manager.sign("hello");
        assert!(manager.verify("hello", &sig));
        assert!(!manager.verify("world", &sig));
    }
}

use crate::error::NanowebError;
use hmac::{Hmac, Mac};
use rand::Rng;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type HmacSha256 = Hmac<Sha256>;

pub trait SessionStore: Send + Sync + 'static {
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
    store: Arc<dyn SessionStore>,
    secret: Vec<u8>,
    cookie_name: String,
}

impl SessionManager {
    pub fn new(store: Arc<dyn SessionStore>, secret: &str) -> Self {
        SessionManager {
            store,
            secret: secret.as_bytes().to_vec(),
            cookie_name: "nanoweb_session".to_string(),
        }
    }

    pub fn with_cookie_name(mut self, name: &str) -> Self {
        self.cookie_name = name.to_string();
        self
    }

    fn sign(&self, session_id: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        mac.update(session_id.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    fn verify(&self, session_id: &str, signature: &str) -> bool {
        let expected = self.sign(session_id);
        // Constant-time comparison
        if expected.len() != signature.len() {
            return false;
        }
        expected
            .bytes()
            .zip(signature.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }

    pub fn generate_session_id(&self) -> String {
        let bytes: Vec<u8> = (0..32).map(|_| rand::thread_rng().gen::<u8>()).collect();
        hex::encode(bytes)
    }

    pub fn create_cookie(&self, session_id: &str) -> String {
        let sig = self.sign(session_id);
        let value = format!("{}.{}", session_id, sig);
        format!(
            "{}={}; HttpOnly; Path=/; SameSite=Lax",
            self.cookie_name, value
        )
    }

    pub fn parse_cookie(&self, cookie_header: &str) -> Option<String> {
        let cookie_value = cookie_header.split(';').map(|c| c.trim()).find_map(|c| {
            if let Some(pos) = c.find('=') {
                let key = c[..pos].trim();
                if key == self.cookie_name {
                    Some(c[pos + 1..].trim().to_string())
                } else {
                    None
                }
            } else {
                None
            }
        })?;

        // Split into id.signature
        let dot_pos = cookie_value.rfind('.')?;
        let session_id = &cookie_value[..dot_pos];
        let signature = &cookie_value[dot_pos + 1..];

        if self.verify(session_id, signature) {
            Some(session_id.to_string())
        } else {
            None
        }
    }

    pub fn get_session(&self, cookie_header: Option<&str>) -> Option<HashMap<String, String>> {
        let session_id = self.parse_cookie(cookie_header?)?;
        self.store.get(&session_id)
    }

    pub fn save_session(
        &self,
        session_id: &str,
        data: HashMap<String, String>,
    ) -> Result<(), NanowebError> {
        self.store.set(session_id, data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> SessionManager {
        SessionManager::new(Arc::new(InMemorySessionStore::new()), "test-secret-key")
    }

    #[test]
    fn test_create_and_parse_cookie() {
        let mgr = make_manager();
        let session_id = mgr.generate_session_id();
        let cookie = mgr.create_cookie(&session_id);
        let parsed = mgr.parse_cookie(&cookie.split(';').next().unwrap());
        assert_eq!(parsed, Some(session_id));
    }

    #[test]
    fn test_tampered_cookie_rejected() {
        let mgr = make_manager();
        let bad_cookie = format!("nanoweb_session=fakeid.fakesig");
        let parsed = mgr.parse_cookie(&bad_cookie);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_session_store_roundtrip() {
        let mgr = make_manager();
        let session_id = mgr.generate_session_id();
        let mut data = HashMap::new();
        data.insert("user_id".to_string(), "42".to_string());

        mgr.save_session(&session_id, data.clone()).unwrap();
        let retrieved = mgr.store.get(&session_id).unwrap();
        assert_eq!(retrieved.get("user_id"), Some(&"42".to_string()));
    }
}

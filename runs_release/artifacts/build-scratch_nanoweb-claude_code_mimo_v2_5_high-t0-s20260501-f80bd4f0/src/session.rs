use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub trait SessionStore: Send + Sync {
    fn get(&self, session_id: &str) -> Option<HashMap<String, String>>;
    fn set(&self, session_id: &str, data: HashMap<String, String>);
    fn delete(&self, session_id: &str);
}

#[derive(Clone)]
pub struct SessionManager {
    store: Arc<dyn SessionStore>,
    secret: Vec<u8>,
}

#[allow(dead_code)]
struct SessionData {
    data: HashMap<String, String>,
}

impl SessionManager {
    pub fn new(secret: &str) -> Self {
        Self {
            store: Arc::new(InMemorySessionStore::new()),
            secret: secret.as_bytes().to_vec(),
        }
    }

    pub fn with_store(secret: &str, store: Arc<dyn SessionStore>) -> Self {
        Self {
            store,
            secret: secret.as_bytes().to_vec(),
        }
    }

    pub fn sign(&self, session_id: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        mac.update(session_id.as_bytes());
        let result = mac.finalize().into_bytes();
        use base64::Engine;
        let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(result);
        format!("{session_id}.{signature}")
    }

    pub fn verify(&self, cookie_value: &str) -> Option<String> {
        let parts: Vec<&str> = cookie_value.splitn(2, '.').collect();
        if parts.len() != 2 {
            return None;
        }
        let session_id = parts[0];
        let signature = parts[1];

        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        mac.update(session_id.as_bytes());

        use base64::Engine;
        let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(signature)
            .ok()?;

        mac.verify_slice(&sig_bytes).ok()?;
        Some(session_id.to_string())
    }

    pub fn get_session(&self, req: &crate::http::types::Request) -> HashMap<String, String> {
        let session_id = self.extract_session_id(req);
        match session_id {
            Some(id) => self.store.get(&id).unwrap_or_default(),
            None => HashMap::new(),
        }
    }

    pub fn set_session(&self, session_id: &str, data: HashMap<String, String>) {
        self.store.set(session_id, data);
    }

    pub fn create_session(&self) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.store.set(&id, HashMap::new());
        id
    }

    pub fn cookie_header(&self, session_id: &str) -> String {
        let signed = self.sign(session_id);
        format!("session={signed}; Path=/; HttpOnly; SameSite=Lax")
    }

    fn extract_session_id(&self, req: &crate::http::types::Request) -> Option<String> {
        req.header("cookie").and_then(|cookie_str| {
            cookie_str.split(';').find_map(|pair| {
                let pair = pair.trim();
                if let Some(val) = pair.strip_prefix("session=") {
                    self.verify(val)
                } else {
                    None
                }
            })
        })
    }
}

pub struct InMemorySessionStore {
    sessions: Mutex<HashMap<String, SessionEntry>>,
}

struct SessionEntry {
    data: HashMap<String, String>,
    last_access: Instant,
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

impl SessionStore for InMemorySessionStore {
    fn get(&self, session_id: &str) -> Option<HashMap<String, String>> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(entry) = sessions.get_mut(session_id) {
            entry.last_access = Instant::now();
            Some(entry.data.clone())
        } else {
            None
        }
    }

    fn set(&self, session_id: &str, data: HashMap<String, String>) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(
            session_id.to_string(),
            SessionEntry {
                data,
                last_access: Instant::now(),
            },
        );
    }

    fn delete(&self, session_id: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify() {
        let mgr = SessionManager::new("test-secret");
        let session_id = "abc123";
        let signed = mgr.sign(session_id);
        let verified = mgr.verify(&signed);
        assert_eq!(verified, Some(session_id.to_string()));
    }

    #[test]
    fn verify_bad_signature() {
        let mgr = SessionManager::new("test-secret");
        let result = mgr.verify("abc.badsignature");
        assert!(result.is_none());
    }

    #[test]
    fn session_store_roundtrip() {
        let store = InMemorySessionStore::new();
        let mut data = HashMap::new();
        data.insert("user".into(), "alice".into());
        store.set("s1", data);
        let got = store.get("s1").unwrap();
        assert_eq!(got.get("user").unwrap(), "alice");
    }
}

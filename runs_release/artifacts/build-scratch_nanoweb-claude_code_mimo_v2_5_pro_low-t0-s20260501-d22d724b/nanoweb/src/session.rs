use crate::http::Request;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;

type HmacSha256 = Hmac<Sha256>;

pub trait SessionStore: Send + Sync {
    fn get(&self, session_id: &str) -> Option<HashMap<String, String>>;
    fn set(&self, session_id: &str, data: HashMap<String, String>);
    fn remove(&self, session_id: &str);
    fn create(&self) -> String;
}

pub struct InMemorySessionStore {
    sessions: Mutex<HashMap<String, HashMap<String, String>>>,
    secret: Vec<u8>,
}

impl InMemorySessionStore {
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            secret: secret.into(),
        }
    }

    pub fn sign_session_id(&self, session_id: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key size");
        mac.update(session_id.as_bytes());
        let signature = mac.finalize().into_bytes();
        format!(
            "{}.{}",
            session_id,
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature)
        )
    }

    pub fn verify_session_cookie(&self, cookie: &str) -> Option<String> {
        let parts: Vec<&str> = cookie.splitn(2, '.').collect();
        if parts.len() != 2 {
            return None;
        }
        let session_id = parts[0];
        let signature_b64 = parts[1];

        let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(signature_b64)
            .ok()?;

        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key size");
        mac.update(session_id.as_bytes());
        mac.verify_slice(&signature).ok()?;

        Some(session_id.to_string())
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

    fn remove(&self, session_id: &str) {
        self.sessions.lock().unwrap().remove(session_id);
    }

    fn create(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

pub fn extract_session_id(req: &Request, _store: &dyn SessionStore) -> Option<String> {
    let cookie_header = req.header("cookie")?;
    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(value) = cookie.strip_prefix("session=") {
            return Some(value.to_string());
        }
    }
    None
}

pub fn create_session_cookie(session_id: &str, store: &InMemorySessionStore) -> String {
    let signed = store.sign_session_id(session_id);
    format!("session={}; Path=/; HttpOnly; SameSite=Strict", signed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_create_and_get() {
        let store = InMemorySessionStore::new("test-secret");
        let session_id = store.create();

        let mut data = HashMap::new();
        data.insert("user".to_string(), "alice".to_string());
        store.set(&session_id, data.clone());

        let retrieved = store.get(&session_id).unwrap();
        assert_eq!(retrieved.get("user").unwrap(), "alice");
    }

    #[test]
    fn test_session_sign_and_verify() {
        let store = InMemorySessionStore::new("test-secret");
        let session_id = store.create();
        let signed = store.sign_session_id(&session_id);

        let verified = store.verify_session_cookie(&signed);
        assert_eq!(verified.unwrap(), session_id);
    }

    #[test]
    fn test_session_verify_invalid_signature() {
        let store = InMemorySessionStore::new("test-secret");
        let verified = store.verify_session_cookie("fake-session.invalid-signature");
        assert!(verified.is_none());
    }
}

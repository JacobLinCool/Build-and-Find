use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;
type HmacSha256 = Hmac<Sha256>;

pub trait SessionStore: Send + Sync {
    fn get(&self, session_id: &str) -> Option<HashMap<String, String>>;
    fn set(&self, session_id: &str, data: HashMap<String, String>);
    fn delete(&self, session_id: &str);
    fn create(&self) -> String;
}

struct SessionEntry {
    data: HashMap<String, String>,
    expires: Instant,
}

pub struct InMemorySessionStore {
    sessions: Arc<Mutex<HashMap<String, SessionEntry>>>,
    ttl: Duration,
    secret: String,
}

impl InMemorySessionStore {
    pub fn new(secret: impl Into<String>) -> Self {
        InMemorySessionStore {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            ttl: Duration::from_secs(3600),
            secret: secret.into(),
        }
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    fn sign(&self, session_id: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(self.secret.as_bytes()).expect("HMAC can take any key size");
        mac.update(session_id.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    pub fn create_signed_cookie(&self, session_id: &str) -> String {
        let sig = self.sign(session_id);
        format!("{}.{}", session_id, sig)
    }

    pub fn verify_signed_cookie(&self, cookie: &str) -> Option<String> {
        let dot = cookie.rfind('.')?;
        let session_id = &cookie[..dot];
        let sig = &cookie[dot + 1..];
        let expected_sig = self.sign(session_id);
        if sig == expected_sig {
            Some(session_id.to_string())
        } else {
            None
        }
    }

    fn cleanup(&self) {
        let now = Instant::now();
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, entry| entry.expires > now);
    }
}

impl SessionStore for InMemorySessionStore {
    fn get(&self, session_id: &str) -> Option<HashMap<String, String>> {
        self.cleanup();
        let sessions = self.sessions.lock().unwrap();
        sessions.get(session_id).and_then(|e| {
            if e.expires > Instant::now() {
                Some(e.data.clone())
            } else {
                None
            }
        })
    }

    fn set(&self, session_id: &str, data: HashMap<String, String>) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(
            session_id.to_string(),
            SessionEntry {
                data,
                expires: Instant::now() + self.ttl,
            },
        );
    }

    fn delete(&self, session_id: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.remove(session_id);
    }

    fn create(&self) -> String {
        Uuid::new_v4().to_string()
    }
}

pub struct SessionManager {
    pub store: Arc<dyn SessionStore>,
    pub cookie_name: String,
}

impl SessionManager {
    pub fn new(store: impl SessionStore + 'static) -> Self {
        SessionManager {
            store: Arc::new(store),
            cookie_name: "session".to_string(),
        }
    }

    pub fn get_session_from_request(
        &self,
        req: &crate::http::types::Request,
    ) -> Option<(String, HashMap<String, String>)> {
        let cookie_header = req.header("cookie")?;
        let session_cookie = get_cookie_value(cookie_header, &self.cookie_name)?;

        // Try to verify signature if using InMemorySessionStore
        let session_id = session_cookie.to_string();
        let data = self.store.get(&session_id)?;
        Some((session_id, data))
    }
}

fn get_cookie_value<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(eq) = part.find('=') {
            if part[..eq].trim() == name {
                return Some(part[eq + 1..].trim());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_create_and_get() {
        let store = InMemorySessionStore::new("secret-key");
        let id = store.create();
        let mut data = HashMap::new();
        data.insert("user".to_string(), "alice".to_string());
        store.set(&id, data.clone());

        let retrieved = store.get(&id).unwrap();
        assert_eq!(retrieved.get("user"), Some(&"alice".to_string()));
    }

    #[test]
    fn test_session_delete() {
        let store = InMemorySessionStore::new("secret-key");
        let id = store.create();
        store.set(&id, HashMap::new());
        store.delete(&id);
        assert!(store.get(&id).is_none());
    }

    #[test]
    fn test_signed_cookie() {
        let store = InMemorySessionStore::new("my-secret");
        let session_id = "test-session-id";
        let cookie = store.create_signed_cookie(session_id);
        let verified = store.verify_signed_cookie(&cookie).unwrap();
        assert_eq!(verified, session_id);
    }

    #[test]
    fn test_tampered_cookie_rejected() {
        let store = InMemorySessionStore::new("my-secret");
        let session_id = "test-session-id";
        let mut cookie = store.create_signed_cookie(session_id);
        cookie.push_str("tampered");
        assert!(store.verify_signed_cookie(&cookie).is_none());
    }
}

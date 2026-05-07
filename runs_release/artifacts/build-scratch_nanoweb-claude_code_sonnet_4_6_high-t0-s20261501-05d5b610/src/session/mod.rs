use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

use crate::http::request::Request;
use crate::http::response::{CookieOptions, Response};

type HmacSha256 = Hmac<Sha256>;

pub const SESSION_COOKIE: &str = "nanoweb_session";

/// Per-session data store.
pub trait SessionStore: Send + Sync {
    fn create(&self) -> String;
    fn get(&self, id: &str) -> Option<HashMap<String, String>>;
    fn set(&self, id: &str, data: HashMap<String, String>);
    fn delete(&self, id: &str);
}

/// In-memory session store (default implementation).
pub struct MemorySessionStore {
    sessions: RwLock<HashMap<String, HashMap<String, String>>>,
}

impl MemorySessionStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            sessions: RwLock::new(HashMap::new()),
        })
    }
}

impl Default for MemorySessionStore {
    fn default() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }
}

impl SessionStore for MemorySessionStore {
    fn create(&self) -> String {
        let id = Uuid::new_v4().to_string();
        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(id.clone(), HashMap::new());
        id
    }

    fn get(&self, id: &str) -> Option<HashMap<String, String>> {
        let sessions = self.sessions.read().unwrap();
        sessions.get(id).cloned()
    }

    fn set(&self, id: &str, data: HashMap<String, String>) {
        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(id.to_string(), data);
    }

    fn delete(&self, id: &str) {
        let mut sessions = self.sessions.write().unwrap();
        sessions.remove(id);
    }
}

/// Session manager that handles signing and verification.
pub struct SessionManager {
    store: Arc<dyn SessionStore>,
    secret: Vec<u8>,
    cookie_name: String,
    max_age: i64,
}

impl SessionManager {
    pub fn new(store: Arc<dyn SessionStore>, secret: impl AsRef<[u8]>) -> Self {
        Self {
            store,
            secret: secret.as_ref().to_vec(),
            cookie_name: SESSION_COOKIE.to_string(),
            max_age: 3600 * 24, // 24 hours
        }
    }

    pub fn with_cookie_name(mut self, name: impl Into<String>) -> Self {
        self.cookie_name = name.into();
        self
    }

    pub fn with_max_age(mut self, seconds: i64) -> Self {
        self.max_age = seconds;
        self
    }

    fn sign(&self, value: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).unwrap();
        mac.update(value.as_bytes());
        let result = mac.finalize().into_bytes();
        hex::encode(result)
    }

    fn verify_and_extract<'a>(&self, cookie: &'a str) -> Option<&'a str> {
        // Format: "session_id.signature"
        let dot = cookie.rfind('.')?;
        let id = &cookie[..dot];
        let sig = &cookie[dot + 1..];
        let expected = self.sign(id);
        if sig == expected {
            Some(id)
        } else {
            None
        }
    }

    fn create_signed_cookie(&self, session_id: &str) -> String {
        let sig = self.sign(session_id);
        format!("{}.{}", session_id, sig)
    }

    pub fn get_session(&self, req: &Request) -> Option<(String, HashMap<String, String>)> {
        let cookie = req.cookie(&self.cookie_name)?;
        let session_id = self.verify_and_extract(&cookie)?.to_string();
        let data = self.store.get(&session_id)?;
        Some((session_id, data))
    }

    pub fn create_session(&self) -> (String, String) {
        let session_id = self.store.create();
        let cookie = self.create_signed_cookie(&session_id);
        (session_id, cookie)
    }

    pub fn set_session_data(&self, session_id: &str, data: HashMap<String, String>) {
        self.store.set(session_id, data);
    }

    pub fn delete_session(&self, session_id: &str, response: &mut Response) {
        self.store.delete(session_id);
        let opts = CookieOptions::new().path("/").max_age(0);
        response.set_cookie(&self.cookie_name, "", &opts);
    }

    pub fn set_session_cookie(&self, session_id: &str, response: &mut Response) {
        let cookie_value = self.create_signed_cookie(session_id);
        let opts = CookieOptions::new()
            .http_only()
            .path("/")
            .max_age(self.max_age)
            .same_site("Lax");
        response.set_cookie(&self.cookie_name, &cookie_value, &opts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Headers, Method, Version};

    fn make_manager() -> SessionManager {
        SessionManager::new(MemorySessionStore::new(), b"test-secret-key")
    }

    #[test]
    fn test_create_and_retrieve_session() {
        let mgr = make_manager();
        let (session_id, cookie) = mgr.create_session();
        assert!(!session_id.is_empty());
        assert!(cookie.contains(&session_id));

        let mut data = HashMap::new();
        data.insert("user".to_string(), "alice".to_string());
        mgr.set_session_data(&session_id, data);

        // Simulate request with cookie
        let mut req = Request::new(
            Method::Get,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        req.headers
            .insert("Cookie", format!("nanoweb_session={}", cookie));

        let (id, retrieved) = mgr.get_session(&req).unwrap();
        assert_eq!(id, session_id);
        assert_eq!(retrieved.get("user").unwrap(), "alice");
    }

    #[test]
    fn test_tampered_session_rejected() {
        let mgr = make_manager();
        let (session_id, mut cookie) = mgr.create_session();
        // Tamper with the cookie
        cookie = format!("tampered-id.{}", cookie.split('.').nth(1).unwrap_or(""));

        let mut req = Request::new(
            Method::Get,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        req.headers
            .insert("Cookie", format!("nanoweb_session={}", cookie));

        // Should fail to verify
        assert!(mgr.get_session(&req).is_none());
        let _ = session_id; // suppress warning
    }

    #[test]
    fn test_missing_session_returns_none() {
        let mgr = make_manager();
        let req = Request::new(
            Method::Get,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        assert!(mgr.get_session(&req).is_none());
    }
}

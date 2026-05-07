use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;

type HmacSha256 = Hmac<Sha256>;

/// Pluggable session store trait.
pub trait SessionStore: Send + Sync {
    fn get(&self, session_id: &str) -> Option<HashMap<String, String>>;
    fn set(&self, session_id: &str, data: HashMap<String, String>);
    fn delete(&self, session_id: &str);
    fn create(&self) -> String;
}

/// In-memory session store.
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

impl SessionStore for InMemorySessionStore {
    fn get(&self, session_id: &str) -> Option<HashMap<String, String>> {
        let sessions = self.sessions.lock().unwrap();
        sessions.get(session_id).cloned()
    }

    fn set(&self, session_id: &str, data: HashMap<String, String>) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(session_id.to_string(), data);
    }

    fn delete(&self, session_id: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.remove(session_id);
    }

    fn create(&self) -> String {
        use rand::Rng;
        let session_id: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(session_id.clone(), HashMap::new());
        session_id
    }
}

/// Sign a session ID with HMAC-SHA256.
pub fn sign_session(session_id: &str, secret: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(session_id.as_bytes());
    let signature = mac.finalize().into_bytes();
    let sig_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature);
    format!("{}.{}", session_id, sig_b64)
}

/// Verify a signed session cookie. Returns the session ID if valid.
pub fn verify_session(signed: &str, secret: &[u8]) -> Option<String> {
    let dot_pos = signed.rfind('.')?;
    let session_id = &signed[..dot_pos];
    let sig_b64 = &signed[dot_pos + 1..];

    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(sig_b64)
        .ok()?;

    let mut mac = HmacSha256::new_from_slice(secret).ok()?;
    mac.update(session_id.as_bytes());
    mac.verify_slice(&signature).ok()?;

    Some(session_id.to_string())
}

/// Parse the session cookie from the Cookie header.
pub fn parse_session_cookie(cookie_header: &str, cookie_name: &str) -> Option<String> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix(&format!("{}=", cookie_name)) {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_session_store() {
        let store = InMemorySessionStore::new();
        let id = store.create();
        assert!(!id.is_empty());

        let mut data = HashMap::new();
        data.insert("user".into(), "alice".into());
        store.set(&id, data.clone());

        let retrieved = store.get(&id).unwrap();
        assert_eq!(retrieved.get("user").unwrap(), "alice");

        store.delete(&id);
        assert!(store.get(&id).is_none());
    }

    #[test]
    fn test_sign_and_verify() {
        let secret = b"super-secret-key";
        let session_id = "abc123";

        let signed = sign_session(session_id, secret);
        assert!(signed.starts_with("abc123."));

        let verified = verify_session(&signed, secret);
        assert_eq!(verified.unwrap(), session_id);
    }

    #[test]
    fn test_verify_tampered_cookie() {
        let secret = b"super-secret-key";
        let signed = sign_session("abc123", secret);
        let tampered = signed.replace("abc123", "evil");
        assert!(verify_session(&tampered, secret).is_none());
    }

    #[test]
    fn test_verify_wrong_secret() {
        let signed = sign_session("abc123", b"secret1");
        assert!(verify_session(&signed, b"secret2").is_none());
    }

    #[test]
    fn test_parse_session_cookie() {
        let header = "session_id=abc123; other=value";
        assert_eq!(
            parse_session_cookie(header, "session_id"),
            Some("abc123".into())
        );
    }
}

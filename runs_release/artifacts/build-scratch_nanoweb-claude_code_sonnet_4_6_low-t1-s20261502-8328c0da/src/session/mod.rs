use crate::error::NanowebError;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// Session data
#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub data: HashMap<String, String>,
    pub created_at: Instant,
}

impl Session {
    pub fn new(id: String) -> Self {
        Session {
            id,
            data: HashMap::new(),
            created_at: Instant::now(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(|s| s.as_str())
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.data.insert(key.into(), value.into());
    }
}

/// Pluggable session store
pub trait SessionStore: Send + Sync {
    fn get<'a>(
        &'a self,
        session_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<Session>> + Send + 'a>>;

    fn set<'a>(
        &'a self,
        session: Session,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), NanowebError>> + Send + 'a>>;

    fn delete<'a>(
        &'a self,
        session_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), NanowebError>> + Send + 'a>>;
}

/// In-memory session store
pub struct MemorySessionStore {
    sessions: Arc<RwLock<HashMap<String, (Session, Instant)>>>,
    ttl: Duration,
}

impl MemorySessionStore {
    pub fn new(ttl: Duration) -> Self {
        MemorySessionStore {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }
}

impl Default for MemorySessionStore {
    fn default() -> Self {
        MemorySessionStore::new(Duration::from_secs(3600))
    }
}

impl SessionStore for MemorySessionStore {
    fn get<'a>(
        &'a self,
        session_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<Session>> + Send + 'a>> {
        Box::pin(async move {
            let sessions = self.sessions.read().await;
            if let Some((session, expiry)) = sessions.get(session_id) {
                if Instant::now() < *expiry {
                    return Some(session.clone());
                }
            }
            None
        })
    }

    fn set<'a>(
        &'a self,
        session: Session,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), NanowebError>> + Send + 'a>> {
        Box::pin(async move {
            let expiry = Instant::now() + self.ttl;
            let mut sessions = self.sessions.write().await;
            sessions.insert(session.id.clone(), (session, expiry));
            Ok(())
        })
    }

    fn delete<'a>(
        &'a self,
        session_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), NanowebError>> + Send + 'a>> {
        Box::pin(async move {
            let mut sessions = self.sessions.write().await;
            sessions.remove(session_id);
            Ok(())
        })
    }
}

/// Session manager with HMAC signing
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

    pub fn sign(&self, session_id: &str, data: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .expect("HMAC can take any key size");
        mac.update(session_id.as_bytes());
        mac.update(b":");
        mac.update(data.as_bytes());
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }

    pub fn verify_signature(&self, session_id: &str, data: &str, signature: &str) -> bool {
        let expected = self.sign(session_id, data);
        expected == signature
    }

    pub fn create_session_id(&self) -> String {
        Uuid::new_v4().to_string()
    }

    pub fn create_cookie(&self, session: &Session) -> String {
        let data = serde_json::to_string(&session.data).unwrap_or_default();
        let signature = self.sign(&session.id, &data);
        let value = format!("{}.{}.{}", session.id, base64_encode(&data), signature);
        format!(
            "{}={}; Path=/; HttpOnly; SameSite=Strict",
            self.cookie_name, value
        )
    }

    pub async fn get_session(&self, cookie_header: Option<&str>) -> Option<Session> {
        let cookies = cookie_header?;
        let value = extract_cookie(cookies, &self.cookie_name)?;

        let parts: Vec<&str> = value.splitn(3, '.').collect();
        if parts.len() != 3 {
            return None;
        }

        let session_id = parts[0];
        let data_b64 = parts[1];
        let signature = parts[2];

        let data = base64_decode(data_b64)?;
        if !self.verify_signature(session_id, &data, signature) {
            return None;
        }

        self.store.get(session_id).await
    }

    pub async fn save_session(&self, session: Session) -> Result<String, NanowebError> {
        let cookie = self.create_cookie(&session);
        self.store.set(session).await?;
        Ok(cookie)
    }

    pub fn new_session(&self) -> Session {
        Session::new(self.create_session_id())
    }
}

fn extract_cookie(cookies: &str, name: &str) -> Option<String> {
    for part in cookies.split(';') {
        let part = part.trim();
        if let Some(eq_pos) = part.find('=') {
            if &part[..eq_pos] == name {
                return Some(part[eq_pos + 1..].to_string());
            }
        }
    }
    None
}

fn base64_encode(s: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s)
}

fn base64_decode(s: &str) -> Option<String> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_store() {
        let store = MemorySessionStore::default();
        let mut session = Session::new("test-id".to_string());
        session.set("user", "alice");

        store.set(session.clone()).await.unwrap();
        let retrieved = store.get("test-id").await.unwrap();
        assert_eq!(retrieved.get("user"), Some("alice"));
    }

    #[test]
    fn test_session_signing() {
        let store = Arc::new(MemorySessionStore::default());
        let manager = SessionManager::new(store, "secret");
        let sig = manager.sign("session-1", "data");
        assert!(manager.verify_signature("session-1", "data", &sig));
        assert!(!manager.verify_signature("session-2", "data", &sig));
    }
}

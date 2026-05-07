use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine;
use hmac::{Hmac, Mac};
use parking_lot::Mutex;
use rand::RngCore;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use sha2::Sha256;

use crate::error::Result;

type HmacSha256 = Hmac<Sha256>;

pub type SessionData = Map<String, Value>;

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn load(&self, session_id: &str) -> Result<Option<SessionData>>;
    async fn save(&self, session_id: &str, data: SessionData) -> Result<()>;
    async fn delete(&self, session_id: &str) -> Result<()>;
}

#[derive(Default)]
pub struct MemorySessionStore {
    sessions: Mutex<HashMap<String, SessionData>>,
}

impl MemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SessionStore for MemorySessionStore {
    async fn load(&self, session_id: &str) -> Result<Option<SessionData>> {
        Ok(self.sessions.lock().get(session_id).cloned())
    }

    async fn save(&self, session_id: &str, data: SessionData) -> Result<()> {
        self.sessions.lock().insert(session_id.to_string(), data);
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<()> {
        self.sessions.lock().remove(session_id);
        Ok(())
    }
}

#[derive(Clone)]
pub struct Session {
    inner: Arc<Mutex<SessionInner>>,
    store: Arc<dyn SessionStore>,
    secret: Arc<Vec<u8>>,
    cookie_name: Arc<String>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id())
            .finish_non_exhaustive()
    }
}

struct SessionInner {
    id: String,
    data: SessionData,
    dirty: bool,
}

impl Session {
    pub async fn from_cookies(
        cookies: &HashMap<String, String>,
        cookie_name: impl Into<String>,
        store: Arc<dyn SessionStore>,
        secret: Vec<u8>,
    ) -> Result<Self> {
        let cookie_name = cookie_name.into();
        let secret = Arc::new(secret);
        let (id, is_new) = if let Some(cookie) = cookies.get(&cookie_name) {
            match verify_signed_cookie(cookie, &secret) {
                Some(session_id) => (session_id, false),
                None => (generate_session_id(), true),
            }
        } else {
            (generate_session_id(), true)
        };

        let data = if is_new {
            SessionData::new()
        } else {
            store.load(&id).await?.unwrap_or_default()
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(SessionInner {
                id,
                data,
                dirty: false,
            })),
            store,
            secret,
            cookie_name: Arc::new(cookie_name),
        })
    }

    pub fn id(&self) -> String {
        self.inner.lock().id.clone()
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.inner
            .lock()
            .data
            .get(key)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    pub fn insert<T: serde::Serialize>(&self, key: impl Into<String>, value: T) -> Result<()> {
        let mut inner = self.inner.lock();
        inner.data.insert(key.into(), serde_json::to_value(value)?);
        inner.dirty = true;
        Ok(())
    }

    pub fn remove(&self, key: &str) {
        let mut inner = self.inner.lock();
        inner.data.remove(key);
        inner.dirty = true;
    }

    pub fn signed_cookie(&self) -> String {
        let id = self.inner.lock().id.clone();
        sign_signed_cookie(&id, &self.secret)
    }

    pub async fn commit(&self) -> Result<Option<String>> {
        let (id, data, should_save) = {
            let inner = self.inner.lock();
            if !inner.dirty {
                return Ok(None);
            }
            (inner.id.clone(), inner.data.clone(), true)
        };
        if !should_save {
            return Ok(None);
        }
        self.store.save(&id, data).await?;
        Ok(Some(format!(
            "{}={}; Path=/; HttpOnly; SameSite=Lax",
            self.cookie_name,
            sign_signed_cookie(&id, &self.secret)
        )))
    }
}

fn generate_session_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn sign_signed_cookie(session_id: &str, secret: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac key");
    mac.update(session_id.as_bytes());
    let signature =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    format!("{session_id}.{signature}")
}

fn verify_signed_cookie(cookie: &str, secret: &[u8]) -> Option<String> {
    let (id, signature) = cookie.rsplit_once('.')?;
    let mut mac = HmacSha256::new_from_slice(secret).ok()?;
    mac.update(id.as_bytes());
    let expected =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    if expected == signature {
        Some(id.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn signs_and_verifies_cookie() {
        let signed = sign_signed_cookie("abc", b"secret");
        assert_eq!(verify_signed_cookie(&signed, b"secret"), Some("abc".into()));
    }

    #[tokio::test]
    async fn stores_data_in_memory() {
        let store = Arc::new(MemorySessionStore::new());
        let session = Session::from_cookies(&HashMap::new(), "session", store, b"secret".to_vec())
            .await
            .unwrap();
        session.insert("name", "alice").unwrap();
        let cookie = session.commit().await.unwrap().unwrap();
        assert!(cookie.contains("session="));
    }
}

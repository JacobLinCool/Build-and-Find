use crate::error::Result;
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use serde_json::Value;
use sha2::Sha256;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

type HmacSha256 = Hmac<Sha256>;

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn get(&self, id: &str) -> Result<Option<Value>>;
    async fn set(&self, id: &str, value: Value) -> Result<()>;
    async fn remove(&self, id: &str) -> Result<()>;
}

#[derive(Default)]
pub struct MemorySessionStore {
    inner: RwLock<HashMap<String, Value>>,
}

#[async_trait]
impl SessionStore for MemorySessionStore {
    async fn get(&self, id: &str) -> Result<Option<Value>> {
        Ok(self.inner.read().unwrap().get(id).cloned())
    }

    async fn set(&self, id: &str, value: Value) -> Result<()> {
        self.inner.write().unwrap().insert(id.to_string(), value);
        Ok(())
    }

    async fn remove(&self, id: &str) -> Result<()> {
        self.inner.write().unwrap().remove(id);
        Ok(())
    }
}

#[derive(Clone)]
pub struct SessionManager {
    secret: Arc<Vec<u8>>,
    pub cookie_name: String,
    store: Arc<dyn SessionStore>,
}

impl SessionManager {
    pub fn new(
        secret: Vec<u8>,
        cookie_name: impl Into<String>,
        store: Arc<dyn SessionStore>,
    ) -> Self {
        Self {
            secret: Arc::new(secret),
            cookie_name: cookie_name.into(),
            store,
        }
    }

    pub fn default_with_secret(secret: impl Into<Vec<u8>>) -> Self {
        Self::new(
            secret.into(),
            "nanoweb_session",
            Arc::new(MemorySessionStore::default()),
        )
    }

    pub fn generate_session_id(&self) -> String {
        let mut bytes = [0u8; 16];
        OsRng.fill_bytes(&mut bytes);
        hex_encode(&bytes)
    }

    pub fn sign_session_id(&self, id: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("valid hmac key");
        mac.update(id.as_bytes());
        let sig = mac.finalize().into_bytes();
        format!("{}.{}", id, hex_encode(&sig))
    }

    pub fn verify_signed_session(&self, signed: &str) -> Option<String> {
        let (id, signature) = signed.rsplit_once('.')?;
        let mut mac = HmacSha256::new_from_slice(&self.secret).ok()?;
        mac.update(id.as_bytes());
        let expected = mac.finalize().into_bytes();
        if hex_encode(&expected) == signature {
            Some(id.to_string())
        } else {
            None
        }
    }

    pub async fn load_signed(&self, signed: &str) -> Result<Option<Value>> {
        if let Some(id) = self.verify_signed_session(signed) {
            self.store.get(&id).await
        } else {
            Ok(None)
        }
    }

    pub async fn save_session(&self, id: &str, value: Value) -> Result<()> {
        self.store.set(id, value).await
    }

    pub async fn cookie_for(&self, id: &str) -> Result<String> {
        Ok(format!(
            "{}={}; HttpOnly; Path=/; SameSite=Lax",
            self.cookie_name,
            self.sign_session_id(id)
        ))
    }

    pub async fn issue_new_session(&self, value: Value) -> Result<(String, String)> {
        let id = self.generate_session_id();
        self.save_session(&id, value).await?;
        let cookie = self.cookie_for(&id).await?;
        Ok((id, cookie))
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{:02x}", byte));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::SessionManager;
    #[test]
    fn sign_and_verify() {
        let manager = SessionManager::default_with_secret(b"secret".to_vec());
        let signed = manager.sign_session_id("abc");
        assert_eq!(
            manager.verify_signed_session(&signed),
            Some("abc".to_string())
        );
    }
}

use crate::http::Request;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use rand::{Rng, distributions::Alphanumeric};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;

type HmacSha256 = Hmac<Sha256>;

pub trait SessionStore: Send + Sync {
    fn get(&self, id: &str) -> Option<HashMap<String, String>>;
    fn set(&self, id: &str, data: HashMap<String, String>);
    fn delete(&self, id: &str);
}

#[derive(Default)]
pub struct MemorySessionStore {
    inner: Mutex<HashMap<String, HashMap<String, String>>>,
}

impl SessionStore for MemorySessionStore {
    fn get(&self, id: &str) -> Option<HashMap<String, String>> {
        self.inner.lock().unwrap().get(id).cloned()
    }

    fn set(&self, id: &str, data: HashMap<String, String>) {
        self.inner.lock().unwrap().insert(id.to_string(), data);
    }

    fn delete(&self, id: &str) {
        self.inner.lock().unwrap().remove(id);
    }
}

#[derive(Clone)]
pub struct SessionManager {
    secret: Vec<u8>,
}

impl SessionManager {
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self {
            secret: secret.into(),
        }
    }

    pub fn new_id(&self) -> String {
        rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect()
    }

    pub fn cookie_value(&self, id: &str) -> String {
        format!("{id}.{}", self.sign(id))
    }

    pub fn extract_id(&self, req: &Request) -> Option<String> {
        let raw = req.cookie("nanoweb_session")?;
        let (id, sig) = raw.split_once('.')?;
        (self.sign(id) == sig).then(|| id.to_string())
    }

    fn sign(&self, id: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("hmac accepts any key length");
        mac.update(id.as_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_cookie() {
        let m = SessionManager::new("secret");
        let v = m.cookie_value("abc");
        assert!(v.starts_with("abc."));
    }
}

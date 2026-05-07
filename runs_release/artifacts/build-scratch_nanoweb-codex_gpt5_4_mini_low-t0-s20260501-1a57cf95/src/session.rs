use crate::error::NanowebError;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type HmacSha256 = Hmac<Sha256>;

pub trait SessionStore: Send + Sync {
    fn get(&self, key: &str) -> Option<HashMap<String, String>>;
    fn set(&self, key: String, value: HashMap<String, String>);
}

#[derive(Clone, Default)]
pub struct MemorySessionStore(Arc<Mutex<HashMap<String, HashMap<String, String>>>>);

impl SessionStore for MemorySessionStore {
    fn get(&self, key: &str) -> Option<HashMap<String, String>> {
        self.0.lock().unwrap().get(key).cloned()
    }
    fn set(&self, key: String, value: HashMap<String, String>) {
        self.0.lock().unwrap().insert(key, value);
    }
}

pub fn sign_cookie(secret: &str, session_id: &str) -> Result<String, NanowebError> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| NanowebError::Server(e.to_string()))?;
    mac.update(session_id.as_bytes());
    Ok(format!(
        "{}.{}",
        session_id,
        hex::encode(mac.finalize().into_bytes())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signs() {
        assert!(sign_cookie("s", "id").unwrap().contains('.'));
    }
}

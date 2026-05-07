use crate::http::{Request, Response};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use rand::{Rng, distributions::Alphanumeric};
use serde_json::Value;
use sha2::Sha256;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

type HmacSha256 = Hmac<Sha256>;
pub type SessionData = HashMap<String, Value>;

pub trait SessionStore: Send + Sync {
    fn load(&self, id: &str) -> Option<SessionData>;
    fn save(&self, id: &str, data: SessionData);
    fn delete(&self, id: &str);
}

#[derive(Default)]
pub struct MemorySessionStore {
    sessions: Mutex<HashMap<String, SessionData>>,
}

impl SessionStore for MemorySessionStore {
    fn load(&self, id: &str) -> Option<SessionData> {
        self.sessions
            .lock()
            .expect("session mutex poisoned")
            .get(id)
            .cloned()
    }

    fn save(&self, id: &str, data: SessionData) {
        self.sessions
            .lock()
            .expect("session mutex poisoned")
            .insert(id.to_string(), data);
    }

    fn delete(&self, id: &str) {
        self.sessions
            .lock()
            .expect("session mutex poisoned")
            .remove(id);
    }
}

#[derive(Clone)]
pub struct SessionManager {
    secret: Vec<u8>,
    store: Arc<dyn SessionStore>,
}

impl SessionManager {
    pub fn new(secret: impl Into<Vec<u8>>, store: Arc<dyn SessionStore>) -> Self {
        Self {
            secret: secret.into(),
            store,
        }
    }

    pub fn memory(secret: impl Into<Vec<u8>>) -> Self {
        Self::new(secret, Arc::new(MemorySessionStore::default()))
    }

    pub fn get_or_create(
        &self,
        request: &Request,
        response: &mut Response,
    ) -> (String, SessionData) {
        if let Some(id) = self.session_id(request) {
            let data = self.store.load(&id).unwrap_or_default();
            return (id, data);
        }
        let id = random_id();
        response.set_header(
            "Set-Cookie",
            format!(
                "nw_session={}; HttpOnly; SameSite=Lax; Path=/",
                self.sign_cookie(&id)
            ),
        );
        (id, SessionData::new())
    }

    pub fn save(&self, id: &str, data: SessionData) {
        self.store.save(id, data);
    }

    pub fn session_id(&self, request: &Request) -> Option<String> {
        let signed = request.header("cookie").and_then(|cookie| {
            cookie.split(';').find_map(|part| {
                let (name, value) = part.trim().split_once('=')?;
                (name == "nw_session").then_some(value)
            })
        })?;
        self.verify_cookie(signed)
    }

    fn sign_cookie(&self, id: &str) -> String {
        format!("{id}.{}", self.signature(id))
    }

    fn verify_cookie(&self, signed: &str) -> Option<String> {
        let (id, sig) = signed.rsplit_once('.')?;
        (self.signature(id) == sig).then(|| id.to_string())
    }

    fn signature(&self, id: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("hmac accepts any key");
        mac.update(id.as_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }
}

fn random_id() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;

    #[test]
    fn signed_cookie_round_trips() {
        let manager = SessionManager::memory("secret");
        let mut req = Request::new(Method::Get, "/");
        let mut res = Response::text(200, "");
        let (id, _) = manager.get_or_create(&req, &mut res);
        req.set_header(
            "Cookie",
            res.header("set-cookie").unwrap().split(';').next().unwrap(),
        );
        assert_eq!(manager.session_id(&req), Some(id));
    }
}

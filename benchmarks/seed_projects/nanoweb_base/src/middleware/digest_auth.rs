//! RFC 7616 "Digest" authentication middleware.
//!
//! This middleware is retained for compatibility with older clients.
//! New deployments should prefer `BasicAuthMiddleware` over TLS or
//! session cookies.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::NanowebError;
use crate::http::types::{Request, Response, StatusCode};
use crate::middleware::{Middleware, MiddlewareOutcome};

/// How long a server-issued `nonce` is honoured before being rejected.
pub const DEFAULT_NONCE_TTL: Duration = Duration::from_secs(300);

pub struct DigestAuthConfig {
    pub realm: String,
    /// Users keyed by username, value is the plain password. A real
    /// deployment would store HA1 = MD5(user:realm:password) directly.
    pub users: HashMap<String, String>,
    pub nonce_ttl: Duration,
    pub opaque: String,
}

impl DigestAuthConfig {
    pub fn new(realm: impl Into<String>, users: HashMap<String, String>) -> Self {
        Self {
            realm: realm.into(),
            users,
            nonce_ttl: DEFAULT_NONCE_TTL,
            opaque: "nanoweb-digest".to_string(),
        }
    }
}

pub struct DigestAuthMiddleware {
    pub config: DigestAuthConfig,
    issued: Mutex<HashMap<String, Instant>>,
    counter: AtomicU64,
}

impl DigestAuthMiddleware {
    pub fn new(config: DigestAuthConfig) -> Self {
        Self {
            config,
            issued: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(0),
        }
    }

    fn mint_nonce(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let ts = Instant::now();
        let nonce = format!("nonce-{:016x}", n);
        self.issued
            .lock()
            .expect("digest issued mutex poisoned")
            .insert(nonce.clone(), ts);
        nonce
    }

    fn is_nonce_valid(&self, nonce: &str) -> bool {
        let mut issued = self.issued.lock().expect("digest issued mutex poisoned");
        if let Some(ts) = issued.get(nonce).copied() {
            if ts.elapsed() <= self.config.nonce_ttl {
                return true;
            }
            issued.remove(nonce);
        }
        false
    }

    fn challenge(&self) -> Response {
        let nonce = self.mint_nonce();
        let mut resp = Response::status(StatusCode::UNAUTHORIZED);
        let val = format!(
            "Digest realm=\"{}\", qop=\"auth\", nonce=\"{}\", opaque=\"{}\", algorithm=MD5",
            self.config.realm, nonce, self.config.opaque,
        );
        resp.headers.set("www-authenticate", val);
        resp
    }

    fn check_response(
        &self,
        params: &HashMap<String, String>,
        method: &str,
        uri: &str,
    ) -> bool {
        let (Some(user), Some(realm), Some(nonce), Some(resp), Some(qop), Some(nc), Some(cnonce)) = (
            params.get("username"),
            params.get("realm"),
            params.get("nonce"),
            params.get("response"),
            params.get("qop"),
            params.get("nc"),
            params.get("cnonce"),
        ) else {
            return false;
        };
        if realm != &self.config.realm {
            return false;
        }
        if !self.is_nonce_valid(nonce) {
            return false;
        }
        let Some(password) = self.config.users.get(user) else {
            return false;
        };
        let ha1 = md5_hex(&format!("{}:{}:{}", user, self.config.realm, password));
        let ha2 = md5_hex(&format!("{}:{}", method, uri));
        let expect =
            md5_hex(&format!("{}:{}:{}:{}:{}:{}", ha1, nonce, nc, cnonce, qop, ha2));
        expect.eq_ignore_ascii_case(resp)
    }
}

fn parse_digest_header(raw: &str) -> Option<HashMap<String, String>> {
    let rest = raw
        .strip_prefix("Digest ")
        .or_else(|| raw.strip_prefix("digest "))?;
    let mut out = HashMap::new();
    let mut chars = rest.chars().peekable();
    while chars.peek().is_some() {
        // skip whitespace and commas
        while let Some(&c) = chars.peek() {
            if c == ',' || c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' {
                chars.next();
                break;
            }
            key.push(c);
            chars.next();
        }
        let mut value = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                if c == '"' {
                    break;
                }
                value.push(c);
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ',' {
                    break;
                }
                value.push(c);
                chars.next();
            }
        }
        if !key.is_empty() {
            out.insert(key.trim().to_ascii_lowercase(), value);
        }
    }
    Some(out)
}

/// Tiny pure-Rust MD5. Digest authentication is only legacy here, so
/// this avoids pulling in an extra crypto crate just for a checksum.
fn md5_hex(input: &str) -> String {
    let digest = md5_compute(input.as_bytes());
    let mut s = String::with_capacity(32);
    for b in digest.iter() {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn md5_compute(msg: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    let original_len_bits = (msg.len() as u64).wrapping_mul(8);
    let mut padded: Vec<u8> = msg.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&original_len_bits.to_le_bytes());

    for chunk in padded.chunks(64) {
        let mut m = [0u32; 16];
        for (i, word) in chunk.chunks(4).enumerate() {
            m[i] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (mut f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            f = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

impl Middleware for DigestAuthMiddleware {
    fn name(&self) -> &'static str {
        "digest_auth"
    }

    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<MiddlewareOutcome, NanowebError>> + Send + 'a>>
    {
        Box::pin(async move {
            let raw = match req.headers.get("authorization") {
                Some(v) => v.to_string(),
                None => return Ok(MiddlewareOutcome::Respond(self.challenge())),
            };
            let params = match parse_digest_header(&raw) {
                Some(p) => p,
                None => return Ok(MiddlewareOutcome::Respond(self.challenge())),
            };
            let method = req.method.as_str();
            let uri = params
                .get("uri")
                .cloned()
                .unwrap_or_else(|| req.target.clone());
            if self.check_response(&params, method, &uri) {
                Ok(MiddlewareOutcome::Continue(req))
            } else {
                Ok(MiddlewareOutcome::Respond(self.challenge()))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::Method;

    #[test]
    fn md5_of_empty() {
        assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn md5_of_known_input() {
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn parser_handles_quoted_values() {
        let h = r#"Digest username="alice", realm="r", nonce="n1", uri="/x", qop=auth, nc=00000001, cnonce="c1", response="rrr", algorithm=MD5"#;
        let params = parse_digest_header(h).unwrap();
        assert_eq!(params.get("username").unwrap(), "alice");
        assert_eq!(params.get("qop").unwrap(), "auth");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_header_challenges() {
        let mut users = HashMap::new();
        users.insert("alice".into(), "secret".into());
        let m = DigestAuthMiddleware::new(DigestAuthConfig::new("realm", users));
        let req = Request::new(Method::Get, "/x");
        match m.before(req).await.unwrap() {
            MiddlewareOutcome::Respond(r) => {
                let www = r.headers.get("www-authenticate").unwrap();
                assert!(www.starts_with("Digest "));
            }
            _ => panic!("expected challenge"),
        }
    }
}

//! Legacy INI config loader.
//!
//! Retained for compatibility with older deployments that have not yet
//! migrated to TOML. New installations should set
//! `--config-format toml` (the default) and prefer the richer schema
//! the TOML loader exposes.

use std::collections::HashMap;

use crate::config::{
    DigestAuthConfigFile, DigestUserEntry, NanowebConfig, RateLimitConfigFile, ServerConfig,
    SessionConfigFile,
};
use crate::error::NanowebError;

/// A flat [section] -> key -> value structure. INI has no native lists,
/// so multi-value sections (e.g. digest users) are expressed as repeated
/// `user.NAME = password` entries inside `[digest_auth]`.
type Ini = HashMap<String, HashMap<String, String>>;

fn tokenise(text: &str) -> Result<Ini, NanowebError> {
    let mut out: Ini = HashMap::new();
    let mut section = String::from("");
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            let end = line
                .find(']')
                .ok_or_else(|| NanowebError::Config(format!("ini: unterminated section at line {}", lineno + 1)))?;
            section = line[1..end].trim().to_string();
            out.entry(section.clone()).or_default();
            continue;
        }
        let eq = line
            .find('=')
            .ok_or_else(|| NanowebError::Config(format!("ini: missing '=' at line {}", lineno + 1)))?;
        let key = line[..eq].trim().to_string();
        let value = line[eq + 1..].trim().trim_matches('"').to_string();
        out.entry(section.clone()).or_default().insert(key, value);
    }
    Ok(out)
}

fn get(ini: &Ini, section: &str, key: &str) -> Option<String> {
    ini.get(section).and_then(|m| m.get(key)).cloned()
}

fn parse_bool(s: &str) -> Option<bool> {
    Some(match s.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => true,
        "false" | "no" | "off" | "0" => false,
        _ => return None,
    })
}

pub fn parse(text: &str) -> Result<NanowebConfig, NanowebError> {
    let ini = tokenise(text)?;
    let mut cfg = NanowebConfig::default();

    // [server]
    let mut server = ServerConfig::default();
    if let Some(v) = get(&ini, "server", "listen") {
        server.listen = v;
    }
    if let Some(v) = get(&ini, "server", "log_level") {
        server.log_level = v;
    }
    if let Some(v) = get(&ini, "server", "static_dir") {
        server.static_dir = Some(std::path::PathBuf::from(v));
    }
    if let Some(v) = get(&ini, "server", "metrics") {
        server.metrics = parse_bool(&v).ok_or_else(|| {
            NanowebError::Config(format!("ini: bad bool for metrics: {}", v))
        })?;
    }
    if let Some(v) = get(&ini, "server", "graceful_timeout_s") {
        server.graceful_timeout_s = v
            .parse::<u64>()
            .map_err(|_| NanowebError::Config("ini: bad graceful_timeout_s".into()))?;
    }
    if let Some(v) = get(&ini, "server", "tls_cert") {
        server.tls_cert = Some(std::path::PathBuf::from(v));
    }
    if let Some(v) = get(&ini, "server", "tls_key") {
        server.tls_key = Some(std::path::PathBuf::from(v));
    }
    cfg.server = server;

    // [rate_limit]
    let mut rl = RateLimitConfigFile::default();
    if let Some(v) = get(&ini, "rate_limit", "enabled") {
        rl.enabled =
            parse_bool(&v).ok_or_else(|| NanowebError::Config("ini: bad rate_limit.enabled".into()))?;
    }
    if let Some(v) = get(&ini, "rate_limit", "capacity") {
        rl.capacity = v
            .parse::<f64>()
            .map_err(|_| NanowebError::Config("ini: bad rate_limit.capacity".into()))?;
    }
    if let Some(v) = get(&ini, "rate_limit", "refill_per_sec") {
        rl.refill_per_sec = v
            .parse::<f64>()
            .map_err(|_| NanowebError::Config("ini: bad rate_limit.refill_per_sec".into()))?;
    }
    cfg.rate_limit = rl;

    // [session]
    let mut session = SessionConfigFile::default();
    if let Some(v) = get(&ini, "session", "secret") {
        session.secret = v;
    }
    if let Some(v) = get(&ini, "session", "ttl_seconds") {
        session.ttl_seconds = v
            .parse::<u64>()
            .map_err(|_| NanowebError::Config("ini: bad session.ttl_seconds".into()))?;
    }
    cfg.session = session;

    // [digest_auth]
    let mut digest = DigestAuthConfigFile::default();
    if let Some(v) = get(&ini, "digest_auth", "enabled") {
        digest.enabled =
            parse_bool(&v).ok_or_else(|| NanowebError::Config("ini: bad digest_auth.enabled".into()))?;
    }
    if let Some(v) = get(&ini, "digest_auth", "realm") {
        digest.realm = v;
    }
    if let Some(section) = ini.get("digest_auth") {
        for (k, v) in section {
            if let Some(username) = k.strip_prefix("user.") {
                digest.users.push(DigestUserEntry {
                    username: username.to_string(),
                    password: v.clone(),
                });
            }
        }
    }
    cfg.digest_auth = digest;

    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_and_values() {
        let raw = r#"
; comment
[server]
listen = "0.0.0.0:9000"
log_level = debug

[rate_limit]
enabled = false
capacity = 10
refill_per_sec = 2

[digest_auth]
enabled = true
realm = "legacy"
user.alice = secret
user.bob = other
"#;
        let cfg = parse(raw).unwrap();
        assert_eq!(cfg.server.listen, "0.0.0.0:9000");
        assert!(!cfg.rate_limit.enabled);
        assert!(cfg.digest_auth.enabled);
        assert_eq!(cfg.digest_auth.users.len(), 2);
    }

    #[test]
    fn missing_sections_fall_back_to_defaults() {
        let cfg = parse("").unwrap();
        assert_eq!(cfg.server.listen, "127.0.0.1:8080");
    }
}

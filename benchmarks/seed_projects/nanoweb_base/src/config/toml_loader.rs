//! TOML config loader.

use crate::config::NanowebConfig;
use crate::error::NanowebError;

pub fn parse(text: &str) -> Result<NanowebConfig, NanowebError> {
    toml::from_str::<NanowebConfig>(text)
        .map_err(|e| NanowebError::Config(format!("toml: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_toml() {
        let raw = r#"
[server]
listen = "0.0.0.0:7000"
log_level = "debug"

[rate_limit]
enabled = false
capacity = 10
refill_per_sec = 1
"#;
        let cfg = parse(raw).unwrap();
        assert_eq!(cfg.server.listen, "0.0.0.0:7000");
        assert_eq!(cfg.server.log_level, "debug");
        assert!(!cfg.rate_limit.enabled);
    }

    #[test]
    fn defaults_are_populated() {
        let cfg = parse("").unwrap();
        assert_eq!(cfg.server.listen, "127.0.0.1:8080");
    }
}

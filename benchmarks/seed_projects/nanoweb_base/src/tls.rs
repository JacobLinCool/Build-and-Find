//! TLS wiring. The bulk of this module is behind the `tls` Cargo
//! feature. When the feature is off the crate still compiles and
//! `build_config` returns a clear `FeatureDisabled` error so the CLI
//! `--tls-cert` flag can fail gracefully.

use crate::error::NanowebError;

#[cfg(feature = "tls")]
pub use enabled::*;

#[cfg(not(feature = "tls"))]
pub use disabled::*;

#[cfg(feature = "tls")]
mod enabled {
    use super::*;
    use std::fs::File;
    use std::io::BufReader;
    use std::path::Path;
    use std::sync::Arc;

    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls::ServerConfig;

    /// Build a rustls `ServerConfig` from PEM-encoded files.
    pub fn build_config(cert_path: &Path, key_path: &Path) -> Result<Arc<ServerConfig>, NanowebError> {
        let certs = load_certs(cert_path)?;
        let key = load_key(key_path)?;
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| NanowebError::Tls(format!("rustls: {}", e)))?;
        Ok(Arc::new(config))
    }

    fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, NanowebError> {
        let mut reader = BufReader::new(
            File::open(path).map_err(|e| NanowebError::Tls(format!("open cert: {}", e)))?,
        );
        let mut out = Vec::new();
        for cert in rustls_pemfile::certs(&mut reader) {
            match cert {
                Ok(c) => out.push(c),
                Err(e) => {
                    return Err(NanowebError::Tls(format!("parse cert: {}", e)));
                }
            }
        }
        if out.is_empty() {
            return Err(NanowebError::Tls("no certificates found".into()));
        }
        Ok(out)
    }

    fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>, NanowebError> {
        let mut reader = BufReader::new(
            File::open(path).map_err(|e| NanowebError::Tls(format!("open key: {}", e)))?,
        );
        for item in rustls_pemfile::read_all(&mut reader) {
            match item {
                Ok(rustls_pemfile::Item::Pkcs8Key(key)) => return Ok(key.into()),
                Ok(rustls_pemfile::Item::Pkcs1Key(key)) => return Ok(key.into()),
                Ok(rustls_pemfile::Item::Sec1Key(key)) => return Ok(key.into()),
                Ok(_) => continue,
                Err(e) => return Err(NanowebError::Tls(format!("parse key: {}", e))),
            }
        }
        Err(NanowebError::Tls("no private key found in file".into()))
    }
}

#[cfg(not(feature = "tls"))]
mod disabled {
    use super::*;
    use std::path::Path;

    /// Stand-in so call sites compile without the feature. Always
    /// returns `FeatureDisabled` to inform users what to do.
    pub fn build_config(_cert: &Path, _key: &Path) -> Result<(), NanowebError> {
        Err(NanowebError::FeatureDisabled("tls"))
    }
}

/// Check whether the optional TLS inputs can be honoured. Returns
/// a descriptive `FeatureDisabled` error when the feature is off but
/// TLS materials were provided on the command line.
pub fn require_if_requested(cert: Option<&str>, key: Option<&str>) -> Result<(), NanowebError> {
    #[cfg(feature = "tls")]
    {
        let _ = (cert, key);
        Ok(())
    }
    #[cfg(not(feature = "tls"))]
    {
        if cert.is_some() || key.is_some() {
            return Err(NanowebError::FeatureDisabled("tls"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "tls"))]
    #[test]
    fn requires_feature_when_asked() {
        assert!(require_if_requested(Some("a"), None).is_err());
    }

    #[cfg(feature = "tls")]
    #[test]
    fn feature_enabled_accepts_flags() {
        assert!(require_if_requested(Some("a"), Some("b")).is_ok());
    }
}

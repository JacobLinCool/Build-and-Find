//! TLS support, behind the `tls` Cargo feature.

use crate::error::{NanowebError, Result};

#[cfg(feature = "tls")]
mod imp {
    use super::*;
    use std::path::Path;
    use std::sync::Arc;

    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls::ServerConfig;

    pub fn load_server_config(cert_path: &Path, key_path: &Path) -> Result<Arc<ServerConfig>> {
        let cert_pem =
            std::fs::read(cert_path).map_err(|e| NanowebError::Tls(format!("read cert: {}", e)))?;
        let key_pem =
            std::fs::read(key_path).map_err(|e| NanowebError::Tls(format!("read key: {}", e)))?;
        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_pem.as_slice())
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| NanowebError::Tls(format!("parse cert: {}", e)))?;
        if certs.is_empty() {
            return Err(NanowebError::Tls("no certificates in PEM".into()));
        }
        let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut key_pem.as_slice())
            .map_err(|e| NanowebError::Tls(format!("parse key: {}", e)))?
            .ok_or_else(|| NanowebError::Tls("no private key in PEM".into()))?;
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| NanowebError::Tls(format!("server config: {}", e)))?;
        Ok(Arc::new(config))
    }
}

#[cfg(feature = "tls")]
pub use imp::load_server_config;

#[cfg(not(feature = "tls"))]
pub fn fail_disabled() -> Result<()> {
    Err(NanowebError::Tls(
        "TLS support is not compiled in (build with --features tls)".into(),
    ))
}

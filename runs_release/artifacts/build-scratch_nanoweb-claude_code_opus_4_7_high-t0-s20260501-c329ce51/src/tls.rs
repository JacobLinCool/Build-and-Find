use crate::error::{NanowebError, Result};
use std::path::Path;

#[cfg(feature = "tls")]
pub use tls_impl::*;

#[cfg(feature = "tls")]
mod tls_impl {
    use super::*;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls::ServerConfig;
    use std::fs::File;
    use std::io::BufReader;
    use std::sync::Arc;

    pub fn load_server_config<P: AsRef<Path>, Q: AsRef<Path>>(
        cert_path: P,
        key_path: Q,
    ) -> Result<Arc<ServerConfig>> {
        let cert_file = File::open(cert_path.as_ref())
            .map_err(|e| NanowebError::Tls(format!("opening cert: {}", e)))?;
        let mut cert_reader = BufReader::new(cert_file);
        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
            .collect::<std::io::Result<_>>()
            .map_err(|e| NanowebError::Tls(format!("parsing certs: {}", e)))?;
        if certs.is_empty() {
            return Err(NanowebError::Tls("no certificates found in PEM".into()));
        }
        let key_file = File::open(key_path.as_ref())
            .map_err(|e| NanowebError::Tls(format!("opening key: {}", e)))?;
        let mut key_reader = BufReader::new(key_file);
        let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|e| NanowebError::Tls(format!("reading key: {}", e)))?
            .ok_or_else(|| NanowebError::Tls("no private key found in PEM".into()))?;
        let cfg = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| NanowebError::Tls(format!("server config: {}", e)))?;
        Ok(Arc::new(cfg))
    }
}

#[cfg(not(feature = "tls"))]
pub fn ensure_disabled<P: AsRef<Path>, Q: AsRef<Path>>(_c: P, _k: Q) -> Result<()> {
    Err(NanowebError::Tls(
        "TLS support disabled at compile time (build with --features tls)".into(),
    ))
}

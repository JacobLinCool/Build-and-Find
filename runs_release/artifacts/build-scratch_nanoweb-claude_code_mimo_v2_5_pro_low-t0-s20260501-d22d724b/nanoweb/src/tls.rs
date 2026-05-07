use crate::error::NanowebError;

#[cfg(feature = "tls")]
pub mod tls_impl {
    use super::*;
    use rustls::ServerConfig;
    use std::io::BufReader;
    use std::path::Path;
    use std::sync::Arc;
    use tokio_rustls::TlsAcceptor;

    pub fn load_tls_config(
        cert_path: &Path,
        key_path: &Path,
    ) -> Result<Arc<ServerConfig>, NanowebError> {
        let cert_file = std::fs::File::open(cert_path)
            .map_err(|e| NanowebError::Tls(format!("Failed to open cert file: {e}")))?;
        let mut cert_reader = BufReader::new(cert_file);
        let certs = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| NanowebError::Tls(format!("Failed to parse certs: {e}")))?;

        let key_file = std::fs::File::open(key_path)
            .map_err(|e| NanowebError::Tls(format!("Failed to open key file: {e}")))?;
        let mut key_reader = BufReader::new(key_file);
        let key = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|e| NanowebError::Tls(format!("Failed to parse key: {e}")))?
            .ok_or_else(|| NanowebError::Tls("No private key found".into()))?;

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| NanowebError::Tls(format!("TLS config error: {e}")))?;

        Ok(Arc::new(config))
    }

    pub fn create_tls_acceptor(config: Arc<ServerConfig>) -> TlsAcceptor {
        TlsAcceptor::from(config)
    }
}

#[cfg(not(feature = "tls"))]
pub fn tls_not_available() -> Result<(), NanowebError> {
    Err(NanowebError::Tls(
        "TLS support is not enabled. Rebuild with the 'tls' feature enabled.".into(),
    ))
}

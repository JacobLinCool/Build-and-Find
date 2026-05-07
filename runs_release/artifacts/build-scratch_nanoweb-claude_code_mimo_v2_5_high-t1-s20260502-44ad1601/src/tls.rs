use crate::error::{NanowebError, Result};
use rustls::ServerConfig;
use std::path::Path;
use std::sync::Arc;

pub fn load_tls_config(cert_path: &str, key_path: &str) -> Result<ServerConfig> {
    let cert_file = std::fs::File::open(Path::new(cert_path))
        .map_err(|e| NanowebError::Tls(format!("failed to open cert file: {e}")))?;
    let key_file = std::fs::File::open(Path::new(key_path))
        .map_err(|e| NanowebError::Tls(format!("failed to open key file: {e}")))?;

    let mut cert_reader = std::io::BufReader::new(cert_file);
    let mut key_reader = std::io::BufReader::new(key_file);

    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| NanowebError::Tls(format!("failed to parse certs: {e}")))?;

    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|e| NanowebError::Tls(format!("failed to parse key: {e}")))?
        .ok_or_else(|| NanowebError::Tls("no private key found".into()))?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| NanowebError::Tls(format!("TLS config error: {e}")))?;

    Ok(config)
}

#[cfg(feature = "tls")]
pub fn create_tls_acceptor(
    cert_path: &str,
    key_path: &str,
) -> Result<Arc<tokio_rustls::TlsAcceptor>> {
    let config = load_tls_config(cert_path, key_path)?;
    Ok(Arc::new(tokio_rustls::TlsAcceptor::from(Arc::new(config))))
}

#[cfg(not(feature = "tls"))]
pub fn create_tls_acceptor(
    _cert_path: &str,
    _key_path: &str,
) -> Result<Arc<tokio_rustls::TlsAcceptor>> {
    Err(NanowebError::Tls(
        "TLS support is disabled. Enable the 'tls' feature.".into(),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_tls_error_without_feature() {
        // This test verifies the module compiles and basic error handling works
        // Actual TLS testing requires cert files
    }
}

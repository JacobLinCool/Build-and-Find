use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use crate::error::{NanowebError, Result};

#[cfg(feature = "tls")]
pub fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<Arc<rustls::ServerConfig>> {
    let cert_file = File::open(cert_path)
        .map_err(|e| NanowebError::Tls(format!("Cannot open cert file: {}", e)))?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs: Vec<_> = rustls_pemfile::certs(&mut cert_reader)
        .filter_map(|r| r.ok())
        .collect();

    let key_file = File::open(key_path)
        .map_err(|e| NanowebError::Tls(format!("Cannot open key file: {}", e)))?;
    let mut key_reader = BufReader::new(key_file);
    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|e| NanowebError::Tls(format!("Cannot parse key: {}", e)))?
        .ok_or_else(|| NanowebError::Tls("No private key found".into()))?;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| NanowebError::Tls(format!("TLS config error: {}", e)))?;

    Ok(Arc::new(config))
}

#[cfg(not(feature = "tls"))]
pub fn load_tls_config(_cert_path: &Path, _key_path: &Path) -> Result<Arc<rustls::ServerConfig>> {
    Err(NanowebError::Tls(
        "TLS support is not enabled. Rebuild with the `tls` feature.".into(),
    ))
}

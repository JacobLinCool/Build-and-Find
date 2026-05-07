#![cfg(feature = "tls")]

use crate::error::{NanowebError, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

pub fn load_acceptor(cert: &Path, key: &Path) -> Result<TlsAcceptor> {
    let cert_file =
        std::fs::File::open(cert).map_err(|e| NanowebError::Tls(format!("open cert: {}", e)))?;
    let mut br = std::io::BufReader::new(cert_file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut br)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| NanowebError::Tls(format!("parse certs: {}", e)))?;
    if certs.is_empty() {
        return Err(NanowebError::Tls("no certificates found".into()));
    }
    let key_file =
        std::fs::File::open(key).map_err(|e| NanowebError::Tls(format!("open key: {}", e)))?;
    let mut br = std::io::BufReader::new(key_file);
    let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut br)
        .map_err(|e| NanowebError::Tls(format!("parse key: {}", e)))?
        .ok_or_else(|| NanowebError::Tls("no private key found".into()))?;
    let cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| NanowebError::Tls(format!("server config: {}", e)))?;
    Ok(TlsAcceptor::from(Arc::new(cfg)))
}

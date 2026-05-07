use crate::error::{NanowebError, Result};
use std::path::Path;

#[cfg(feature = "tls")]
pub use tokio_rustls::TlsAcceptor;

#[cfg(feature = "tls")]
use rustls::{
    pki_types::{
        CertificateDer, PrivateKeyDer, PrivatePkcs1KeyDer, PrivatePkcs8KeyDer, PrivateSec1KeyDer,
    },
    ServerConfig,
};

#[cfg(feature = "tls")]
use std::{fs::File, io::BufReader, sync::Arc};

#[cfg(feature = "tls")]
pub fn load_acceptor(cert_path: &Path, key_path: &Path) -> Result<TlsAcceptor> {
    let certs = load_certs(cert_path)?;
    let key = load_private_key(key_path)?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|err| NanowebError::Tls(format!("{err}")))?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

#[cfg(feature = "tls")]
fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|err| NanowebError::Tls(format!("failed to read certificates: {err}")))?;
    Ok(certs)
}

#[cfg(feature = "tls")]
fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut keys = rustls_pemfile::pkcs8_private_keys(&mut reader)
        .collect::<std::result::Result<Vec<PrivatePkcs8KeyDer<'static>>, _>>()
        .map_err(|err| NanowebError::Tls(format!("failed to read private key: {err}")))?;
    if let Some(key) = keys.pop() {
        return Ok(PrivateKeyDer::from(key));
    }
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut keys = rustls_pemfile::rsa_private_keys(&mut reader)
        .collect::<std::result::Result<Vec<PrivatePkcs1KeyDer<'static>>, _>>()
        .map_err(|err| NanowebError::Tls(format!("failed to read private key: {err}")))?;
    if let Some(key) = keys.pop() {
        return Ok(PrivateKeyDer::from(key));
    }
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut keys = rustls_pemfile::ec_private_keys(&mut reader)
        .collect::<std::result::Result<Vec<PrivateSec1KeyDer<'static>>, _>>()
        .map_err(|err| NanowebError::Tls(format!("failed to read private key: {err}")))?;
    if let Some(key) = keys.pop() {
        return Ok(PrivateKeyDer::from(key));
    }
    Err(NanowebError::Tls("no private key found".to_string()))
}

#[cfg(not(feature = "tls"))]
pub struct TlsAcceptor;

#[cfg(not(feature = "tls"))]
pub fn load_acceptor(_cert_path: &Path, _key_path: &Path) -> Result<TlsAcceptor> {
    Err(NanowebError::Tls(
        "tls support is disabled at compile time; rebuild with the tls feature enabled".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn compile_time_tls_stub_exists() {
        assert!(true);
    }
}

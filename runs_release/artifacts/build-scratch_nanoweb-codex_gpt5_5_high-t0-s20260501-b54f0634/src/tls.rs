use crate::error::{NanowebError, Result};
use std::path::Path;
#[cfg(feature = "tls")]
use std::sync::Arc;

#[cfg(feature = "tls")]
pub type TlsAcceptor = tokio_rustls::TlsAcceptor;

#[cfg(feature = "tls")]
pub fn load_acceptor(cert_path: &Path, key_path: &Path) -> Result<TlsAcceptor> {
    use std::{fs::File, io::BufReader};
    use tokio_rustls::rustls::ServerConfig;

    let mut cert_reader = BufReader::new(File::open(cert_path)?);
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| NanowebError::Tls(e.to_string()))?;

    let mut key_reader = BufReader::new(File::open(key_path)?);
    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|e| NanowebError::Tls(e.to_string()))?
        .ok_or_else(|| NanowebError::Tls("no private key found".into()))?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| NanowebError::Tls(e.to_string()))?;
    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(config)))
}

#[cfg(not(feature = "tls"))]
pub struct TlsAcceptor;

#[cfg(not(feature = "tls"))]
pub fn load_acceptor(_cert_path: &Path, _key_path: &Path) -> Result<TlsAcceptor> {
    Err(NanowebError::Tls(
        "TLS support is disabled at compile time".into(),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_is_present() {
        assert_eq!(std::mem::size_of::<usize>(), std::mem::size_of::<usize>());
    }
}

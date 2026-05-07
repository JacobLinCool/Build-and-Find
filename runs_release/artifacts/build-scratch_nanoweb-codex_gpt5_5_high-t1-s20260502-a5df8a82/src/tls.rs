use std::path::Path;

use crate::error::{NanowebError, Result};

#[cfg(feature = "tls")]
pub type TlsAcceptor = tokio_rustls::TlsAcceptor;

#[cfg(feature = "tls")]
pub fn load_acceptor(cert_path: &Path, key_path: &Path) -> Result<TlsAcceptor> {
    use std::fs::File;
    use std::io::BufReader;
    use std::sync::Arc;
    use tokio_rustls::rustls::ServerConfig;
    use tokio_rustls::rustls::pki_types::PrivateKeyDer;

    let mut cert_reader = BufReader::new(File::open(cert_path)?);
    let certs =
        rustls_pemfile::certs(&mut cert_reader).collect::<std::result::Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        return Err(NanowebError::Tls(
            "certificate PEM contained no certificates".into(),
        ));
    }

    let mut key_reader = BufReader::new(File::open(key_path)?);
    let mut keys = rustls_pemfile::pkcs8_private_keys(&mut key_reader)
        .map(|key| key.map(PrivateKeyDer::from))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if keys.is_empty() {
        let mut key_reader = BufReader::new(File::open(key_path)?);
        keys = rustls_pemfile::rsa_private_keys(&mut key_reader)
            .map(|key| key.map(PrivateKeyDer::from))
            .collect::<std::result::Result<Vec<_>, _>>()?;
    }
    let key = keys
        .into_iter()
        .next()
        .ok_or_else(|| NanowebError::Tls("private key PEM contained no keys".into()))?;

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
        "TLS support is disabled; rebuild with the tls feature".into(),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn tls_module_compiles() {
        assert!(true);
    }
}

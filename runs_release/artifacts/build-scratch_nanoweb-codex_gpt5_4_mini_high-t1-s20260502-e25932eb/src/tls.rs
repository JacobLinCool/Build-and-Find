use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use crate::error::{NanowebError, Result};

#[cfg(feature = "tls")]
pub fn load_tls_acceptor(cert_path: &str, key_path: &str) -> Result<tokio_rustls::TlsAcceptor> {
    use rustls::ServerConfig;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls_pemfile::{certs, private_key};

    let mut cert_reader = BufReader::new(File::open(cert_path)?);
    let cert_chain: Vec<CertificateDer<'static>> = certs(&mut cert_reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|err| NanowebError::Tls(err.to_string()))?;
    let mut key_reader = BufReader::new(File::open(key_path)?);
    let key: PrivateKeyDer<'static> = private_key(&mut key_reader)
        .map_err(|err| NanowebError::Tls(err.to_string()))?
        .ok_or_else(|| NanowebError::Tls("private key missing".into()))?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .map_err(|err| NanowebError::Tls(err.to_string()))?;
    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(config)))
}

#[cfg(not(feature = "tls"))]
pub fn load_tls_acceptor(_: &str, _: &str) -> Result<()> {
    Err(NanowebError::Tls(
        "tls support is disabled at compile time".into(),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn disabled_feature_returns_error_or_function_exists() {
        #[cfg(not(feature = "tls"))]
        assert!(crate::tls::load_tls_acceptor("a", "b").is_err());
    }
}

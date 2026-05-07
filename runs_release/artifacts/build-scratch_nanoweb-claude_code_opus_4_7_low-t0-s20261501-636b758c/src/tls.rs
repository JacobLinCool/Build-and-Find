//! TLS support via rustls. Behind the `tls` Cargo feature.

#[cfg(feature = "tls")]
mod imp {
    use crate::NanowebError;
    use std::path::Path;
    use std::sync::Arc;

    pub fn load_acceptor(
        cert_path: &Path,
        key_path: &Path,
    ) -> Result<tokio_rustls::TlsAcceptor, NanowebError> {
        let certs = load_certs(cert_path)?;
        let key = load_key(key_path)?;
        let cfg = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| NanowebError::Tls(e.to_string()))?;
        Ok(tokio_rustls::TlsAcceptor::from(Arc::new(cfg)))
    }

    fn load_certs(
        path: &Path,
    ) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, NanowebError> {
        let f = std::fs::File::open(path).map_err(NanowebError::Io)?;
        let mut br = std::io::BufReader::new(f);
        rustls_pemfile::certs(&mut br)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| NanowebError::Tls(e.to_string()))
    }

    fn load_key(path: &Path) -> Result<rustls::pki_types::PrivateKeyDer<'static>, NanowebError> {
        let f = std::fs::File::open(path).map_err(NanowebError::Io)?;
        let mut br = std::io::BufReader::new(f);
        rustls_pemfile::private_key(&mut br)
            .map_err(|e| NanowebError::Tls(e.to_string()))?
            .ok_or_else(|| NanowebError::Tls("no private key in PEM".into()))
    }
}

#[cfg(feature = "tls")]
pub use imp::load_acceptor;

#[cfg(not(feature = "tls"))]
pub fn tls_unsupported() -> crate::NanowebError {
    crate::NanowebError::Tls("TLS support is disabled (compile with --features tls)".into())
}

#[cfg(feature = "tls")]
pub mod acceptor {
    use crate::error::NanowebError;
    use rustls::ServerConfig;
    use rustls_pemfile::{certs, pkcs8_private_keys};
    use std::path::Path;
    use std::sync::Arc;
    use tokio_rustls::TlsAcceptor;

    pub async fn load_tls_acceptor(
        cert_path: &Path,
        key_path: &Path,
    ) -> Result<TlsAcceptor, NanowebError> {
        let cert_file = std::fs::File::open(cert_path)
            .map_err(|e| NanowebError::Tls(format!("failed to open cert: {}", e)))?;
        let key_file = std::fs::File::open(key_path)
            .map_err(|e| NanowebError::Tls(format!("failed to open key: {}", e)))?;

        let mut cert_reader = std::io::BufReader::new(cert_file);
        let mut key_reader = std::io::BufReader::new(key_file);

        let certs: Vec<rustls::pki_types::CertificateDer> = certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| NanowebError::Tls(format!("failed to read certs: {}", e)))?;

        let keys: Vec<rustls::pki_types::PrivateKeyDer> = pkcs8_private_keys(&mut key_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| NanowebError::Tls(format!("failed to read keys: {}", e)))?
            .into_iter()
            .map(rustls::pki_types::PrivateKeyDer::Pkcs8)
            .collect();

        let key = keys
            .into_iter()
            .next()
            .ok_or_else(|| NanowebError::Tls("no private key found".to_string()))?;

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| NanowebError::Tls(e.to_string()))?;

        Ok(TlsAcceptor::from(Arc::new(config)))
    }
}

#[cfg(not(feature = "tls"))]
pub mod acceptor {}

#[cfg(feature = "tls")]
pub mod tls_impl {
    use crate::error::NanowebError;
    use std::path::Path;
    use std::sync::Arc;
    use tokio_rustls::rustls::{self, ServerConfig};
    use tokio_rustls::TlsAcceptor;

    pub fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<TlsAcceptor, NanowebError> {
        use rustls_pemfile::{certs, pkcs8_private_keys};
        use std::io::BufReader;

        let cert_file = std::fs::File::open(cert_path)
            .map_err(|e| NanowebError::TlsError(format!("Cannot open cert: {}", e)))?;
        let key_file = std::fs::File::open(key_path)
            .map_err(|e| NanowebError::TlsError(format!("Cannot open key: {}", e)))?;

        let mut cert_reader = BufReader::new(cert_file);
        let mut key_reader = BufReader::new(key_file);

        let certs: Vec<_> = certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| NanowebError::TlsError(format!("Cannot read certs: {}", e)))?;

        let keys: Vec<_> = pkcs8_private_keys(&mut key_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| NanowebError::TlsError(format!("Cannot read keys: {}", e)))?;

        let key = keys
            .into_iter()
            .next()
            .ok_or_else(|| NanowebError::TlsError("No private key found".to_string()))?;

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, rustls::pki_types::PrivateKeyDer::Pkcs8(key))
            .map_err(|e| NanowebError::TlsError(e.to_string()))?;

        Ok(TlsAcceptor::from(Arc::new(config)))
    }
}

#[cfg(not(feature = "tls"))]
pub mod tls_impl {
    use crate::error::NanowebError;
    use std::path::Path;

    pub fn load_tls_config(_cert: &Path, _key: &Path) -> Result<(), NanowebError> {
        Err(NanowebError::TlsError(
            "TLS feature not enabled. Recompile with --features tls".to_string(),
        ))
    }
}

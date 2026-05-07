use crate::error::NanowebError;

/// TLS configuration using rustls.
#[cfg(feature = "tls")]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
}

#[cfg(feature = "tls")]
impl TlsConfig {
    pub fn new(cert_path: impl Into<String>, key_path: impl Into<String>) -> Self {
        Self {
            cert_path: cert_path.into(),
            key_path: key_path.into(),
        }
    }

    /// Build a rustls ServerConfig from PEM cert and key files.
    pub fn build_server_config(&self) -> Result<rustls::ServerConfig, NanowebError> {
        use rustls::pki_types::CertificateDer;
        use std::io::BufReader;

        let cert_file = std::fs::File::open(&self.cert_path)
            .map_err(|e| NanowebError::Tls(format!("Failed to open cert file: {}", e)))?;
        let mut cert_reader = BufReader::new(cert_file);
        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| NanowebError::Tls(format!("Failed to parse certs: {}", e)))?;

        let key_file = std::fs::File::open(&self.key_path)
            .map_err(|e| NanowebError::Tls(format!("Failed to open key file: {}", e)))?;
        let mut key_reader = BufReader::new(key_file);
        let key = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|e| NanowebError::Tls(format!("Failed to parse key: {}", e)))?
            .ok_or_else(|| NanowebError::Tls("No private key found".into()))?;

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| NanowebError::Tls(format!("TLS config error: {}", e)))?;

        Ok(config)
    }
}

/// Error when TLS feature is not enabled.
#[cfg(not(feature = "tls"))]
pub fn tls_not_available() -> NanowebError {
    NanowebError::Tls("TLS support not compiled in. Enable the 'tls' feature.".into())
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "tls")]
    #[test]
    fn test_tls_config_missing_files() {
        use super::*;
        let config = TlsConfig::new("/nonexistent/cert.pem", "/nonexistent/key.pem");
        assert!(config.build_server_config().is_err());
    }

    #[cfg(not(feature = "tls"))]
    #[test]
    fn test_tls_not_available() {
        use super::*;
        let err = tls_not_available();
        assert!(err.to_string().contains("not compiled"));
    }
}

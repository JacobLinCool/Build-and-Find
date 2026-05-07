#[cfg(feature = "tls")]
pub mod acceptor {
    use crate::error::NanowebError;

    pub fn load_tls_config(
        cert_path: &str,
        key_path: &str,
    ) -> Result<rustls::ServerConfig, NanowebError> {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};

        let cert_file = std::fs::File::open(cert_path)
            .map_err(|e| NanowebError::Tls(format!("Cannot open cert: {}", e)))?;
        let mut cert_reader = std::io::BufReader::new(cert_file);
        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| NanowebError::Tls(format!("Cannot parse cert: {}", e)))?;

        if certs.is_empty() {
            return Err(NanowebError::Tls(
                "No certificates found in PEM file".into(),
            ));
        }

        let key_file = std::fs::File::open(key_path)
            .map_err(|e| NanowebError::Tls(format!("Cannot open key: {}", e)))?;
        let mut key_reader = std::io::BufReader::new(key_file);
        let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|e| NanowebError::Tls(format!("Cannot parse key: {}", e)))?
            .ok_or_else(|| NanowebError::Tls("No private key found in PEM file".into()))?;

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| NanowebError::Tls(format!("TLS config error: {}", e)))?;

        Ok(config)
    }
}

#[cfg(not(feature = "tls"))]
pub mod acceptor {
    use crate::error::NanowebError;

    pub fn load_tls_config(_cert_path: &str, _key_path: &str) -> Result<(), NanowebError> {
        Err(NanowebError::Config(
            "TLS support is not compiled in. Enable the 'tls' feature.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_tls_module_compiles() {
        // This just tests that the module compiles correctly
    }

    #[cfg(not(feature = "tls"))]
    #[test]
    fn test_tls_disabled_returns_error() {
        use super::acceptor;
        let result = acceptor::load_tls_config("cert.pem", "key.pem");
        assert!(result.is_err());
    }
}

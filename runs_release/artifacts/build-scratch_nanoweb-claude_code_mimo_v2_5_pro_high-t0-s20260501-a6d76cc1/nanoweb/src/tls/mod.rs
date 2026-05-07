#[cfg(feature = "tls")]
pub mod config {
    use crate::error::NanowebError;
    use rustls::ServerConfig;
    use std::fs;
    use std::io::BufReader;
    use std::path::Path;
    use std::sync::Arc;

    pub fn load_tls_config(
        cert_path: &Path,
        key_path: &Path,
    ) -> Result<Arc<ServerConfig>, NanowebError> {
        let cert_file = fs::File::open(cert_path)
            .map_err(|e| NanowebError::Tls(format!("failed to open cert: {e}")))?;
        let mut cert_reader = BufReader::new(cert_file);
        let certs = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| NanowebError::Tls(format!("failed to read certs: {e}")))?;

        let key_file = fs::File::open(key_path)
            .map_err(|e| NanowebError::Tls(format!("failed to open key: {e}")))?;
        let mut key_reader = BufReader::new(key_file);
        let key = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|e| NanowebError::Tls(format!("failed to read key: {e}")))?
            .ok_or_else(|| NanowebError::Tls("no private key found".into()))?;

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| NanowebError::Tls(format!("TLS config error: {e}")))?;

        Ok(Arc::new(config))
    }
}

#[cfg(not(feature = "tls"))]
pub mod config {
    use crate::error::NanowebError;
    use std::path::Path;

    pub fn load_tls_config(_cert_path: &Path, _key_path: &Path) -> Result<(), NanowebError> {
        Err(NanowebError::Tls(
            "TLS support not compiled in. Enable the 'tls' feature.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(feature = "tls")]
    fn test_tls_feature_enabled() {
        // Verify the module compiles with TLS support
        assert!(true);
    }
}

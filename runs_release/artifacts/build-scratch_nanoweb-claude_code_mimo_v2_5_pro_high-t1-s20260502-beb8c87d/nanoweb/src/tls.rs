use crate::error::NanowebError;

#[cfg(feature = "tls")]
pub mod tls_inner {
    use crate::error::NanowebError;
    use rustls::ServerConfig;
    use std::io::BufReader;
    use std::path::Path;
    use std::sync::Arc;

    pub fn load_tls_config(
        cert_path: &Path,
        key_path: &Path,
    ) -> Result<Arc<ServerConfig>, NanowebError> {
        let cert_file = std::fs::File::open(cert_path)
            .map_err(|e| NanowebError::Tls(format!("Cannot open cert file: {e}")))?;
        let mut cert_reader = BufReader::new(cert_file);
        let certs = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| NanowebError::Tls(format!("Cannot parse certs: {e}")))?;

        let key_file = std::fs::File::open(key_path)
            .map_err(|e| NanowebError::Tls(format!("Cannot open key file: {e}")))?;
        let mut key_reader = BufReader::new(key_file);
        let key = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|e| NanowebError::Tls(format!("Cannot parse key: {e}")))?
            .ok_or_else(|| NanowebError::Tls("No private key found".to_string()))?;

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| NanowebError::Tls(format!("TLS config error: {e}")))?;

        Ok(Arc::new(config))
    }
}

pub fn check_tls_available() -> Result<(), NanowebError> {
    #[cfg(not(feature = "tls"))]
    {
        Err(NanowebError::Tls(
            "TLS support not compiled in. Rebuild with the 'tls' feature enabled.".to_string(),
        ))
    }
    #[cfg(feature = "tls")]
    {
        Ok(())
    }
}

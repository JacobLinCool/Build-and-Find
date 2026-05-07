#[cfg(feature = "tls")]
pub use tls_impl::*;

#[cfg(feature = "tls")]
mod tls_impl {
    use std::path::Path;
    use std::sync::Arc;

    use rustls::ServerConfig;
    use rustls_pemfile::{certs, pkcs8_private_keys, rsa_private_keys};
    use tokio_rustls::TlsAcceptor;

    use crate::error::NanowebError;

    pub fn load_tls_config(
        cert_path: &Path,
        key_path: &Path,
    ) -> Result<Arc<ServerConfig>, NanowebError> {
        let cert_data = std::fs::read(cert_path)
            .map_err(|e| NanowebError::Tls(format!("Cannot read certificate: {}", e)))?;
        let key_data = std::fs::read(key_path)
            .map_err(|e| NanowebError::Tls(format!("Cannot read key: {}", e)))?;

        // Parse certificates
        let certs: Vec<rustls::pki_types::CertificateDer<'static>> = {
            let mut reader = std::io::BufReader::new(cert_data.as_slice());
            certs(&mut reader)
                .map(|c| c.map(|cert| cert.to_owned()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| NanowebError::Tls(format!("Cannot parse certificates: {}", e)))?
        };

        if certs.is_empty() {
            return Err(NanowebError::Tls(
                "No certificates found in PEM file".into(),
            ));
        }

        // Parse private key (try PKCS8 first, then RSA)
        let key = {
            let mut reader = std::io::BufReader::new(key_data.as_slice());
            let pkcs8: Vec<Vec<u8>> = pkcs8_private_keys(&mut reader)
                .filter_map(|r| r.ok())
                .map(|k| k.secret_pkcs8_der().to_vec())
                .collect();

            if let Some(bytes) = pkcs8.into_iter().next() {
                rustls::pki_types::PrivateKeyDer::Pkcs8(
                    rustls::pki_types::PrivatePkcs8KeyDer::from(bytes),
                )
            } else {
                let mut reader = std::io::BufReader::new(key_data.as_slice());
                let rsa: Vec<Vec<u8>> = rsa_private_keys(&mut reader)
                    .filter_map(|r| r.ok())
                    .map(|k| k.secret_pkcs1_der().to_vec())
                    .collect();
                match rsa.into_iter().next() {
                    Some(bytes) => rustls::pki_types::PrivateKeyDer::Pkcs1(
                        rustls::pki_types::PrivatePkcs1KeyDer::from(bytes),
                    ),
                    None => {
                        return Err(NanowebError::Tls("No private key found in PEM file".into()))
                    }
                }
            }
        };

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| NanowebError::Tls(format!("TLS config error: {}", e)))?;

        Ok(Arc::new(config))
    }

    pub fn make_acceptor(cert_path: &Path, key_path: &Path) -> Result<TlsAcceptor, NanowebError> {
        let config = load_tls_config(cert_path, key_path)?;
        Ok(TlsAcceptor::from(config))
    }
}

/// When TLS feature is disabled, produce a clear error.
#[cfg(not(feature = "tls"))]
pub fn tls_not_available() -> crate::error::NanowebError {
    crate::error::NanowebError::Tls(
        "TLS support is not compiled in. Enable the 'tls' feature.".into(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_tls_feature_compile() {
        // Just verifies this module compiles correctly
        #[cfg(not(feature = "tls"))]
        {
            let e = super::tls_not_available();
            assert!(format!("{}", e).contains("TLS"));
        }
    }
}

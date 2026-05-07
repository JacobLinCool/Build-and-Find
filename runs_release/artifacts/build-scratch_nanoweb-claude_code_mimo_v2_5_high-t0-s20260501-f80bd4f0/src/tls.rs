use crate::error::{NanowebError, Result};
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

pub struct TlsAcceptorWrapper {
    #[cfg(feature = "tls")]
    inner: tokio_rustls::TlsAcceptor,
}

impl TlsAcceptorWrapper {
    pub fn new(cert_path: &str, key_path: &str) -> Result<Self> {
        #[cfg(feature = "tls")]
        {
            let cert_file = File::open(cert_path)
                .map_err(|e| NanowebError::Tls(format!("Failed to open cert file: {e}")))?;
            let key_file = File::open(key_path)
                .map_err(|e| NanowebError::Tls(format!("Failed to open key file: {e}")))?;

            let cert_reader = &mut BufReader::new(cert_file);
            let certs: Vec<_> = rustls_pemfile::certs(cert_reader)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| NanowebError::Tls(format!("Failed to parse certs: {e}")))?
                .into_iter()
                .map(rustls::pki_types::CertificateDer::from)
                .collect();

            let key_reader = &mut BufReader::new(key_file);
            let key_der = rustls_pemfile::private_key(key_reader)
                .map_err(|e| NanowebError::Tls(format!("Failed to parse key: {e}")))?
                .ok_or_else(|| NanowebError::Tls("No private key found".into()))?;

            let config = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, key_der)
                .map_err(|e| NanowebError::Tls(format!("TLS config error: {e}")))?;

            Ok(Self {
                inner: tokio_rustls::TlsAcceptor::from(Arc::new(config)),
            })
        }

        #[cfg(not(feature = "tls"))]
        {
            let _ = (cert_path, key_path);
            Err(NanowebError::Tls(
                "TLS support is not enabled. Recompile with the 'tls' feature.".into(),
            ))
        }
    }

    #[cfg(feature = "tls")]
    pub async fn accept(
        &self,
        stream: tokio::net::TcpStream,
    ) -> std::io::Result<tokio_rustls::server::TlsStream<tokio::net::TcpStream>> {
        self.inner.accept(stream).await
    }
}

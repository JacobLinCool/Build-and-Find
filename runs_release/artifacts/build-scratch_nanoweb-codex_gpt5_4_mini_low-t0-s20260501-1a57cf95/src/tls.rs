use crate::error::NanowebError;
#[cfg(feature = "tls")]
use rustls::ServerConfig;

#[cfg(feature = "tls")]
pub fn load_rustls(cert_path: &str, key_path: &str) -> Result<ServerConfig, NanowebError> {
    use rustls::crypto::ring::default_provider;
    use rustls_pemfile::{certs, private_key};
    use rustls_pki_types::{CertificateDer, PrivateKeyDer};
    use std::fs::File;
    use std::io::BufReader;
    use std::sync::Arc;
    let certs: Vec<CertificateDer<'static>> = certs(&mut BufReader::new(File::open(cert_path)?))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| NanowebError::Tls(e.to_string()))?;
    let key: PrivateKeyDer<'static> = private_key(&mut BufReader::new(File::open(key_path)?))
        .map_err(|e| NanowebError::Tls(e.to_string()))?
        .ok_or_else(|| NanowebError::Tls("no private key found".into()))?;
    Ok(ServerConfig::builder_with_details(
        Arc::new(default_provider()),
        Arc::new(rustls::time_provider::DefaultTimeProvider),
    )
    .with_protocol_versions(&[&rustls::version::TLS13])
    .map_err(|e: rustls::Error| NanowebError::Tls(e.to_string()))?
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .map_err(|e: rustls::Error| NanowebError::Tls(e.to_string()))?)
}

#[cfg(not(feature = "tls"))]
pub fn load_rustls(_: &str, _: &str) -> Result<(), NanowebError> {
    Err(NanowebError::Tls("tls feature disabled".into()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn compiles() {}
}

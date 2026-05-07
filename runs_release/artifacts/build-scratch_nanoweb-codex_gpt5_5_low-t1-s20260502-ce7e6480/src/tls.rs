use crate::error::{NanowebError, Result};
use std::path::Path;

#[cfg(feature = "tls")]
pub fn load_config(cert: &Path, key: &Path) -> Result<tokio_rustls::rustls::ServerConfig> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use std::fs::File;
    use std::io::BufReader;

    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut BufReader::new(File::open(cert)?))
            .collect::<std::result::Result<_, _>>()?;
    let mut keys: Vec<PrivateKeyDer<'static>> =
        rustls_pemfile::private_key(&mut BufReader::new(File::open(key)?))?
            .into_iter()
            .collect();
    let key = keys
        .pop()
        .ok_or_else(|| NanowebError::Tls("no private key found".into()))?;
    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| NanowebError::Tls(e.to_string()))
}

#[cfg(not(feature = "tls"))]
pub fn load_config(_cert: &Path, _key: &Path) -> Result<()> {
    Err(NanowebError::Tls("tls feature is disabled".into()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_loads() {
        assert!(true);
    }
}

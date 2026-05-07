use crate::error::{NanowebError, Result};

#[cfg(feature = "tls")]
pub fn load_config(cert_path: &str, key_path: &str) -> Result<rustls::ServerConfig> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use std::io::BufReader;

    let cert_file = std::fs::File::open(cert_path)?;
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| NanowebError::Tls(e.to_string()))?;
    let key_file = std::fs::File::open(key_path)?;
    let mut keys: Vec<PrivateKeyDer<'static>> =
        rustls_pemfile::private_key(&mut BufReader::new(key_file))
            .map_err(|e| NanowebError::Tls(e.to_string()))?
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
pub fn load_config(_cert_path: &str, _key_path: &str) -> Result<()> {
    Err(NanowebError::Tls(
        "TLS support is disabled; rebuild with the `tls` feature".into(),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_compiles() {
        assert!(true);
    }
}

use crate::error::NanowebError;
#[cfg(feature = "tls")]
pub async fn load_acceptor(
    cert: &str,
    key: &str,
) -> Result<tokio_rustls::TlsAcceptor, NanowebError> {
    use rustls::ServerConfig;
    use rustls_pemfile::{certs, private_key};
    use std::fs::File;
    use std::io::BufReader;
    let certs = certs(&mut BufReader::new(File::open(cert)?))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(rustls::pki_types::CertificateDer::from)
        .collect();
    let key = private_key(&mut BufReader::new(File::open(key)?))?
        .ok_or_else(|| NanowebError::from("missing key"))?;
    let key = match key {
        rustls::pki_types::PrivateKeyDer::Pkcs8(k) => rustls::pki_types::PrivateKeyDer::Pkcs8(k),
        _ => return Err("unsupported private key type".into()),
    };
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(cfg)))
}
#[cfg(not(feature = "tls"))]
pub async fn load_acceptor(_cert: &str, _key: &str) -> Result<(), NanowebError> {
    Err("tls feature disabled".into())
}

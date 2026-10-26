//! Loading a certificate chain and key for rustls.

use std::sync::Arc;

use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::sign::CertifiedKey;
use thiserror::Error;

/// Why a certificate could not be loaded.
#[derive(Debug, Error)]
pub enum CertificateError {
    /// The certificate chain or key is not valid PEM.
    #[error("the {0} is not valid PEM")]
    Pem(&'static str),
    /// The private key is not supported.
    #[error("the private key cannot be used: {0}")]
    Key(rustls::Error),
}

/// Loads a PEM certificate chain and PKCS #8 (or SEC1 / PKCS #1) key for rustls.
///
/// # Errors
///
/// [`CertificateError`] if either is not valid PEM, the chain is empty, or the key type is not
/// supported.
pub fn certified_key(
    chain_pem: &[u8],
    key_pem: &[u8],
) -> Result<Arc<CertifiedKey>, CertificateError> {
    let chain = CertificateDer::pem_slice_iter(chain_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_pem| CertificateError::Pem("certificate chain"))?;
    if chain.is_empty() {
        return Err(CertificateError::Pem("certificate chain"));
    }
    let key =
        PrivateKeyDer::from_pem_slice(key_pem).map_err(|_pem| CertificateError::Pem("key"))?;
    let signing_key = rustls::crypto::aws_lc_rs::default_provider()
        .key_provider
        .load_private_key(key)
        .map_err(CertificateError::Key)?;
    Ok(Arc::new(CertifiedKey::new(chain, signing_key)))
}

#[cfg(test)]
mod tests {
    use zeabur_identity_test_support::certificate::Ca;

    use super::*;

    #[test]
    fn issued_certificate_loads_for_rustls() {
        let issued = Ca::new().unwrap().issue(&["localhost"]).unwrap();
        assert!(certified_key(issued.cert_pem.as_bytes(), issued.key_pem.as_bytes()).is_ok());
        assert!(certified_key(b"", issued.key_pem.as_bytes()).is_err());
        assert!(certified_key(issued.cert_pem.as_bytes(), b"nope").is_err());
    }
}

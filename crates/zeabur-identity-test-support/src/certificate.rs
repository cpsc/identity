//! A CA and serving certificates for tests, in place of the ones cert-manager issues.

use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SanType,
};

/// An ECDSA P-256 CA that keeps its key, as cert-manager's `CA` Issuer does.
pub struct Ca {
    pem: String,
    issuer: Issuer<'static, KeyPair>,
}

impl std::fmt::Debug for Ca {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ca").finish_non_exhaustive()
    }
}

/// A serving certificate and its key (PKCS #8), both PEM.
#[derive(Debug, Clone)]
pub struct Issued {
    /// The serving certificate.
    pub cert_pem: String,
    /// The serving key.
    pub key_pem: String,
}

impl Ca {
    /// Creates a new CA.
    ///
    /// # Errors
    ///
    /// If key generation or signing fails.
    pub fn new() -> Result<Self, rcgen::Error> {
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
        let pem = params.self_signed(&key)?.pem();
        Ok(Self {
            pem,
            issuer: Issuer::new(params, key),
        })
    }

    /// The CA certificate (PEM).
    #[must_use]
    pub fn pem(&self) -> &str {
        &self.pem
    }

    /// Issues a `serverAuth` certificate with a new key for `names`. A name that parses as an IP
    /// address becomes an IP SAN, every other name a DNS SAN.
    ///
    /// # Errors
    ///
    /// If a name is not a valid DNS name, or key generation or signing fails.
    pub fn issue(&self, names: &[&str]) -> Result<Issued, rcgen::Error> {
        let mut leaf = CertificateParams::default();
        leaf.subject_alt_names = names
            .iter()
            .map(|name| {
                name.parse().map_or_else(
                    |_not_ip| (*name).try_into().map(SanType::DnsName),
                    |ip| Ok(SanType::IpAddress(ip)),
                )
            })
            .collect::<Result<_, _>>()?;
        leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        leaf.use_authority_key_identifier_extension = true;
        let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
        let cert_pem = leaf.signed_by(&key, &self.issuer)?.pem();
        Ok(Issued {
            cert_pem,
            key_pem: key.serialize_pem(),
        })
    }
}

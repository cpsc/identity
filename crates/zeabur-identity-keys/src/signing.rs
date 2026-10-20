//! Private signing keys, backed by aws-lc-rs.

use std::fmt;

use aws_lc_rs::encoding::AsDer as _;
use aws_lc_rs::error::{KeyRejected, Unspecified};
use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::rsa::KeySize;
use aws_lc_rs::signature::{
    ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair as _, RSA_PKCS1_SHA256, RsaKeyPair,
};
use thiserror::Error;

use super::Algorithm;
use super::jwk::{Jwk, PublicKey};

/// RSA modulus size for new keys. RFC 7518 §3.3 requires at least 2048 bits; 3072 bits matches
/// the 128-bit security level of P-256.
const RSA_KEY_SIZE: KeySize = KeySize::Rsa3072;

/// A failure in key handling. The cryptographic library reports no further detail.
#[derive(Debug, Error)]
pub enum KeyError {
    /// Key generation failed.
    #[error("key generation failed")]
    Generate(#[source] Unspecified),
    /// Stored key material is not a valid key for the algorithm.
    #[error("stored {alg:?} key is not a valid PKCS #8 key for that algorithm")]
    Parse {
        /// The algorithm the key was stored for.
        alg: Algorithm,
        /// The library's reason, if it gave one.
        #[source]
        source: Option<KeyRejected>,
    },
    /// The library returned a public key in an unexpected form.
    #[error("unexpected public key encoding")]
    PublicKey,
    /// Exporting key material failed.
    #[error("key export failed")]
    Export(#[source] Unspecified),
    /// Signing failed.
    #[error("signing failed")]
    Sign(#[source] Unspecified),
    /// The JWS header or payload could not be serialized.
    #[error("serializing the JWS input failed")]
    Serialize(#[source] serde_json::Error),
}

const fn parse_error(alg: Algorithm) -> impl FnOnce(KeyRejected) -> KeyError {
    move |source| KeyError::Parse {
        alg,
        source: Some(source),
    }
}

enum Inner {
    Es256(EcdsaKeyPair),
    Rs256(RsaKeyPair),
}

/// A private signing key bound to exactly one algorithm (RFC 8725 §3.1).
pub struct SigningKey {
    alg: Algorithm,
    inner: Inner,
    public: PublicKey,
    kid: String,
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print key material.
        f.debug_struct("SigningKey")
            .field("alg", &self.alg)
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

impl SigningKey {
    /// Generates a new key for `alg`.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError::Generate`] if the library fails to generate a key.
    pub fn generate(alg: Algorithm) -> Result<Self, KeyError> {
        let inner = match alg {
            Algorithm::Es256 => Inner::Es256(
                EcdsaKeyPair::generate(&ECDSA_P256_SHA256_FIXED_SIGNING)
                    .map_err(KeyError::Generate)?,
            ),
            Algorithm::Rs256 => {
                Inner::Rs256(RsaKeyPair::generate(RSA_KEY_SIZE).map_err(KeyError::Generate)?)
            }
        };
        Self::from_inner(alg, inner).ok_or(KeyError::PublicKey)
    }

    /// Loads a key from its PKCS #8 v1 DER encoding.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError::Parse`] if `der` is not a valid key for `alg`.
    pub fn from_pkcs8(alg: Algorithm, der: &[u8]) -> Result<Self, KeyError> {
        let inner = match alg {
            Algorithm::Es256 => Inner::Es256(
                EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, der)
                    .map_err(parse_error(alg))?,
            ),
            Algorithm::Rs256 => {
                let key = RsaKeyPair::from_pkcs8(der).map_err(parse_error(alg))?;
                // Refuse undersized keys even if someone stores one by hand (RFC 7518 §3.3).
                if key.public_modulus_len() < 256 {
                    return Err(KeyError::Parse { alg, source: None });
                }
                Inner::Rs256(key)
            }
        };
        Self::from_inner(alg, inner).ok_or(KeyError::PublicKey)
    }

    fn from_inner(alg: Algorithm, inner: Inner) -> Option<Self> {
        let public = match &inner {
            Inner::Es256(key) => {
                // Uncompressed SEC 1 point: 0x04 || X || Y.
                let point = key.public_key().as_ref();
                let (tag, coords) = point.split_first()?;
                if *tag != 0x04 || coords.len() != 64 {
                    return None;
                }
                let (x, y) = coords.split_at(32);
                PublicKey::Ec {
                    x: x.try_into().ok()?,
                    y: y.try_into().ok()?,
                }
            }
            Inner::Rs256(key) => {
                let public = key.public_key();
                PublicKey::Rsa {
                    n: public.modulus().big_endian_without_leading_zero().to_vec(),
                    e: public.exponent().big_endian_without_leading_zero().to_vec(),
                }
            }
        };
        let kid = public.thumbprint();
        Some(Self {
            alg,
            inner,
            public,
            kid,
        })
    }

    /// Exports the key as PKCS #8 v1 DER, for storage.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError::Export`] if the library fails to encode the key.
    pub fn to_pkcs8(&self) -> Result<Vec<u8>, KeyError> {
        match &self.inner {
            Inner::Es256(key) => key
                .to_pkcs8v1()
                .map(|doc| doc.as_ref().to_vec())
                .map_err(KeyError::Export),
            Inner::Rs256(key) => key
                .as_der()
                .map(|der| der.as_ref().to_vec())
                .map_err(KeyError::Export),
        }
    }

    /// The public key.
    #[must_use]
    pub const fn public(&self) -> &PublicKey {
        &self.public
    }

    /// The algorithm this key signs with.
    #[must_use]
    pub const fn alg(&self) -> Algorithm {
        self.alg
    }

    /// The key ID: the RFC 7638 thumbprint of the public key.
    #[must_use]
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// The public key.
    #[must_use]
    pub const fn public_key(&self) -> &PublicKey {
        &self.public
    }

    /// The public JWK.
    #[must_use]
    pub fn jwk(&self) -> Jwk {
        self.public.to_jwk(self.alg)
    }

    /// Signs `message` and returns the JWS signature value: `R || S` for ES256 (RFC 7518 §3.4),
    /// the PKCS #1 v1.5 signature for RS256 (RFC 7518 §3.3).
    ///
    /// # Errors
    ///
    /// Returns [`KeyError::Sign`] if the library fails to sign.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, KeyError> {
        let rng = SystemRandom::new();
        match &self.inner {
            Inner::Es256(key) => key
                .sign(&rng, message)
                .map(|sig| sig.as_ref().to_vec())
                .map_err(KeyError::Sign),
            Inner::Rs256(key) => {
                let mut signature = vec![0; key.public_modulus_len()];
                key.sign(&RSA_PKCS1_SHA256, &rng, message, &mut signature)
                    .map_err(KeyError::Sign)?;
                Ok(signature)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use aws_lc_rs::signature::{
        ECDSA_P256_SHA256_FIXED, RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents,
        UnparsedPublicKey,
    };

    use super::*;

    fn verify(key: &SigningKey, message: &[u8], signature: &[u8]) -> bool {
        match key.public_key() {
            PublicKey::Ec { x, y } => {
                let mut point = vec![0x04];
                point.extend_from_slice(x);
                point.extend_from_slice(y);
                UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, point)
                    .verify(message, signature)
                    .is_ok()
            }
            PublicKey::Rsa { n, e } => RsaPublicKeyComponents { n, e }
                .verify(&RSA_PKCS1_2048_8192_SHA256, message, signature)
                .is_ok(),
        }
    }

    #[test]
    fn generate_sign_verify_and_round_trip() {
        for alg in Algorithm::ALL {
            let key = SigningKey::generate(alg).unwrap();
            let signature = key.sign(b"message").unwrap();
            assert!(verify(&key, b"message", &signature), "{alg:?}");
            assert!(!verify(&key, b"other", &signature), "{alg:?}");

            let reloaded = SigningKey::from_pkcs8(alg, &key.to_pkcs8().unwrap()).unwrap();
            assert_eq!(reloaded.kid(), key.kid());
            assert_eq!(reloaded.public_key(), key.public_key());
        }
    }

    #[test]
    fn es256_signatures_are_fixed_width() {
        let key = SigningKey::generate(Algorithm::Es256).unwrap();
        assert_eq!(key.sign(b"m").unwrap().len(), 64);
    }

    #[test]
    fn rs256_keys_are_3072_bits() {
        let key = SigningKey::generate(Algorithm::Rs256).unwrap();
        let PublicKey::Rsa { n, e } = key.public_key() else {
            panic!("not RSA")
        };
        assert_eq!(n.len(), 384);
        assert_eq!(e, &[1, 0, 1]);
    }

    #[test]
    fn rejects_key_for_the_wrong_algorithm() {
        let es = SigningKey::generate(Algorithm::Es256)
            .unwrap()
            .to_pkcs8()
            .unwrap();
        assert!(matches!(
            SigningKey::from_pkcs8(Algorithm::Rs256, &es).unwrap_err(),
            KeyError::Parse {
                alg: Algorithm::Rs256,
                ..
            }
        ));
        assert!(SigningKey::from_pkcs8(Algorithm::Es256, b"garbage").is_err());
    }

    #[test]
    fn debug_never_prints_key_material() {
        let key = SigningKey::generate(Algorithm::Es256).unwrap();
        let debug = format!("{key:?}");
        assert!(debug.contains(key.kid()));
        assert!(!debug.contains("inner"));
    }
}
